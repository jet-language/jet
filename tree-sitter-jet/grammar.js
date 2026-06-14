// tree-sitter grammar for Jet (M13 C11).
//
// Generated from src/syntax.rs keyword/sigil constants.
// Run `tree-sitter generate` inside this directory after installing the CLI.
//
// Keywords (S1–S10, S16–S24, S26–S30, S37, S41, S50, S55, S57): every
// user-typeable keyword lives in src/syntax.rs with a decision ID (I7).
// This file is the single derived artifact from that source; do not edit
// keyword lists here independently.

module.exports = grammar({
  name: "jet",

  conflicts: ($) => [
    [$._expr, $.lambda_param],
    [$.lambda_expr, $.binary_expr],
    [$.method_call_expr, $.field_expr, $.lambda_expr],
  ],

  extras: ($) => [/\s/, $.comment, $.doc_comment],

  rules: {
    source_file: ($) => repeat($._item),

    // ── Top-level items ────────────────────────────────────────────────────
    _item: ($) =>
      choice(
        $.function_def,
        $.struct_def,
        $.enum_def,
        $.impl_block,
        $.trait_def,
        $.const_def,
        $.import_stmt,
        $.extern_block,
        $.test_block
      ),

    // ── Imports (S16) ──────────────────────────────────────────────────────
    import_stmt: ($) =>
      seq(
        "import",
        field("path", $.string_literal),
        optional(seq("as", field("alias", $.identifier))),
        ";"
      ),

    // ── Extern (S50) ──────────────────────────────────────────────────────
    extern_block: ($) => seq("extern", "rust", $.string_literal, "{", "}"),

    // ── Function definition (S1) ──────────────────────────────────────────
    function_def: ($) =>
      seq(
        optional("pub"),
        "fn",
        field("name", $.identifier),
        optional($.type_params),
        $.param_list,
        optional(seq("->", field("return_type", $.return_type))),
        $.block
      ),

    // ── Struct definition ─────────────────────────────────────────────────
    struct_def: ($) =>
      seq(
        optional("pub"),
        "struct",
        field("name", $.type_identifier),
        optional($.type_params),
        "{",
        repeat($.struct_field),
        repeat($.function_def),
        repeat($.trait_impl_block),
        "}"
      ),

    struct_field: ($) =>
      seq(
        optional("pub"),
        field("name", $.identifier),
        ":",
        field("type", $._type),
        ";"
      ),

    // ── Enum definition (S30) ─────────────────────────────────────────────
    enum_def: ($) =>
      seq(
        optional("pub"),
        "enum",
        field("name", $.type_identifier),
        optional($.type_params),
        "{",
        repeat($.enum_variant),
        repeat($.function_def),
        repeat($.trait_impl_block),
        "}"
      ),

    enum_variant: ($) =>
      seq(
        field("name", $.type_identifier),
        optional(seq("{", repeat($.struct_field), "}")),
        ";"
      ),

    // ── Impl block (S27) ──────────────────────────────────────────────────
    impl_block: ($) =>
      seq(
        "impl",
        field("type", $.type_identifier),
        optional(seq(":", field("trait", $.type_identifier))),
        "{",
        repeat($.function_def),
        "}"
      ),

    trait_impl_block: ($) =>
      seq(
        "impl",
        field("trait", $.type_identifier),
        "{",
        repeat($.function_def),
        "}"
      ),

    // ── Trait definition (S28) ────────────────────────────────────────────
    trait_def: ($) =>
      seq(
        optional("pub"),
        "trait",
        field("name", $.type_identifier),
        "{",
        repeat($.function_def),
        "}"
      ),

    // ── Const definition ──────────────────────────────────────────────────
    const_def: ($) =>
      seq(
        optional("pub"),
        "const",
        field("name", $.identifier),
        ":",
        field("type", $._type),
        "=",
        field("value", $._expr),
        ";"
      ),

    // ── Test block ────────────────────────────────────────────────────────
    test_block: ($) => seq("test", $.string_literal, $.block),

    // ── Type params ───────────────────────────────────────────────────────
    type_params: ($) =>
      seq("<", commaSep1($.type_identifier), ">"),

    // ── Param list ────────────────────────────────────────────────────────
    param_list: ($) =>
      seq("(", commaSep($.param), ")"),

    param: ($) =>
      seq(
        optional(choice("mut", "take", "view", "ref")),
        field("name", $.identifier),
        ":",
        field("type", $._type)
      ),

    // ── Types ─────────────────────────────────────────────────────────────
    _type: ($) =>
      choice(
        $.primitive_type,
        $.type_identifier,
        $.option_type,
        $.fallible_type,
        $.list_type,
        $.map_type,
        $.fn_type
      ),

    primitive_type: (_) => choice("Int", "Float", "Bool", "String", "Char", "Error"),

    type_identifier: ($) => /[A-Z][a-zA-Z0-9_]*/,

    option_type: ($) => seq($._type, "?"),

    fallible_type: ($) => seq($._type, "?", $._type),

    return_type: ($) =>
      choice(
        $.fallible_type,
        seq($._type, "?"),
        seq("(", $.option_type, ")"),
        $._type
      ),

    list_type: ($) => seq("List", "<", $._type, ">"),

    map_type: ($) => seq("Map", "<", $._type, ",", $._type, ">"),

    fn_type: ($) =>
      seq("fn", "(", commaSep($._type), ")", optional(seq("->", $._type))),

    // ── Statements ────────────────────────────────────────────────────────
    block: ($) => seq("{", repeat($._stmt), "}"),

    _stmt: ($) =>
      choice(
        $.val_stmt,      // S2
        $.assign_stmt,
        $.return_stmt,
        $.if_stmt,
        $.while_stmt,
        $.for_stmt,
        $.switch_stmt,   // S24
        $.break_stmt,    // S23
        $.continue_stmt, // S23
        $.loop_stmt,
        $.comptime_stmt, // S57
        $.expr_stmt
      ),

    val_stmt: ($) =>
      seq(
        choice("val", "var"),
        field("name", $.identifier),
        optional(seq(":", field("type", $._type))),
        "=",
        field("value", $._expr),
        ";"
      ),

    assign_stmt: ($) =>
      seq(
        field("target", $._expr),
        choice("=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="),
        field("value", $._expr),
        ";"
      ),

    return_stmt: ($) => seq("return", optional($._expr), ";"),

    if_stmt: ($) =>
      seq(
        "if",
        field("cond", $._expr),
        field("then", $.block),
        optional(seq("else", choice($.if_stmt, $.block)))
      ),

    while_stmt: ($) => seq("while", field("cond", $._expr), $.block),

    for_stmt: ($) =>
      seq(
        "for",
        field("var", $.identifier),
        optional(seq(",", field("var2", $.identifier))),
        "in",
        field("iter", $._expr),
        $.block
      ),

    switch_stmt: ($) =>
      seq(
        "switch",
        field("subject", $._expr),
        "{",
        repeat($.switch_arm),
        optional($.switch_else),
        "}"
      ),

    switch_arm: ($) =>
      seq(field("cond", $._expr), "->", $.block, ";"),

    switch_else: ($) => seq("else", "->", $.block, ";"),

    break_stmt: (_) => seq("break", ";"),

    continue_stmt: (_) => seq("continue", ";"),

    loop_stmt: ($) => seq("loop", $.block),

    comptime_stmt: ($) =>
      seq("comptime", field("name", $.identifier), "=", field("value", $._expr), ";"),

    expr_stmt: ($) => seq($._expr, ";"),

    // ── Expressions (partial — enough for syntax highlighting) ────────────
    _expr: ($) =>
      choice(
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.null_literal,
        $.string_literal,
        $.char_literal,
        $.identifier,
        $.call_expr,
        $.method_call_expr,
        $.field_expr,
        $.lambda_expr,
        $.binary_expr,
        $.unary_expr,
        $.paren_expr,
        $.index_expr
      ),

    integer_literal: (_) => /-?[0-9]+/,
    float_literal: (_) => /-?[0-9]+\.[0-9]*/,
    boolean_literal: (_) => choice("true", "false"),
    null_literal: (_) => "null",
    char_literal: (_) => /'.'/,

    string_literal: ($) =>
      seq(
        '"',
        repeat(choice($.escape_sequence, $.string_interpolation, /[^"\\{]/)),
        '"'
      ),

    escape_sequence: (_) =>
      /\\["\\/bfnrt0]|\\u\{[0-9a-fA-F]+\}/,

    string_interpolation: ($) => seq("{", $._expr, "}"),

    identifier: (_) => /[a-z_][a-zA-Z0-9_]*/,

    call_expr: ($) =>
      seq(field("name", $.identifier), "(", commaSep($._expr), ")"),

    method_call_expr: ($) =>
      seq(
        field("receiver", $._expr),
        ".",
        field("method", $.identifier),
        "(",
        commaSep($._expr),
        ")"
      ),

    field_expr: ($) =>
      seq(field("object", $._expr), ".", field("field", $.identifier)),

    lambda_expr: ($) =>
      seq("(", commaSep($.lambda_param), ")", "=>", choice($.block, $._expr)),

    lambda_param: ($) =>
      seq(field("name", $.identifier), optional(seq(":", $._type))),

    binary_expr: ($) =>
      prec.left(
        seq(
          field("left", $._expr),
          field(
            "op",
            choice(
              "+", "-", "*", "/", "%",
              "&", "|", "^", "<<", ">>",
              "&&", "||",
              "==", "!=", "<", ">", "<=", ">=",
              ".."
            )
          ),
          field("right", $._expr)
        )
      ),

    unary_expr: ($) =>
      prec.right(seq(choice("-", "!"), $._expr)),

    paren_expr: ($) => seq("(", $._expr, ")"),

    index_expr: ($) => seq($._expr, "[", $._expr, "]"),

    // ── Comments (S5, S49) ────────────────────────────────────────────────
    comment: (_) => token(seq("//", /[^/].*/)),
    doc_comment: (_) => token(seq("///", /.*/)),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}
