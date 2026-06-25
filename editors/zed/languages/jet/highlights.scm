; Jet syntax highlighting.
; Node names here MUST match the compiled grammar (editors/tree-sitter/grammar.js →
; src/node-types.json). A reference to a node the grammar doesn't define makes the
; WHOLE language fail to load in Zed ("Invalid node type ..."), so keep this query
; in sync with the grammar whenever keyword/literal nodes change.

; Comments
(line_comment) @comment
(block_comment) @comment
(doc_comment) @comment.doc

; Literals
(string_literal) @string
(multiline_string) @string
(string_interpolation) @string
(escape_sequence) @string.escape
(char_literal) @string.special
(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean
(null_literal) @constant.builtin
(ok_err_literal) @constant.builtin

; Source / dependency refs (`c@system`, `github@owner/repo`)
(source_ref (ref_target) @string.special)

; Types
(primitive_type) @type.builtin
(type_identifier) @type
(generic_type base: (type_identifier) @type)
(capability_sigil) @operator

; Markers / attributes (#Pure, #[Codable], #Caps(...), #grant, ...)
(attribute (marker_name) @attribute)
(attribute_list (marker_name) @attribute)
(lower_marker_name) @attribute
(effect_set (type_identifier) @attribute)
(marker_decl (attribute (marker_name) @attribute))

; Loop labels (@outer)
(loop_label) @label

; Definitions
(function_def name: (identifier) @function)
(extern_fn name: (identifier) @function)
(extern_fn name: (type_identifier) @function)
(call_expr name: (identifier) @function.call)
(turbofish_call name: (identifier) @function.call)
(method_call_expr method: (identifier) @function.method)

; Parameters / fields
(param name: (identifier) @variable.parameter)
(lambda_param name: (identifier) @variable.parameter)
(struct_field name: (identifier) @property)
(field_expr field: (identifier) @property)
(named_arg name: (identifier) @property)

; Self
[
  "self"
] @variable.builtin

; Keywords (the grammar exposes each keyword as its own anonymous token;
; there is no single `keyword` node). `true`/`false` are `boolean_literal`.
[
  "as"
  "extern"
  "fn"
  "impl"
  "module"
  "pub"
  "ref"
  "rust"
  "use"
  "using"
] @keyword

[
  "break"
  "continue"
  "else"
  "if"
  "in"
  "loop"
  "return"
  "step"
] @keyword.control

[
  "comptime"
  "const"
  "derive"
  "distinct"
  "enum"
  "struct"
  "tag"
  "trait"
] @keyword

[
  "assume_deterministic"
  "grant"
  "live"
  "region"
] @keyword

[
  "add"
  "change"
  "migration"
  "remove"
  "rename"
  "via"
] @keyword

; Binding sigils + operators
[
  "@="
  ":="
  "="
  "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
] @operator

[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|" "^" "<<" ">>"
  "->" "=>" ".." "?" "??" "?." "@"
] @operator

; Config / manifest keys (pkg.jet, env.jet)
(config_key) @property

; All other identifiers
(identifier) @variable
