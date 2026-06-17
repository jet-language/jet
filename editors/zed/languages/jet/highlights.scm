; Comments
(comment) @comment
(doc_comment) @comment.doc

; Strings and chars
(string_literal) @string
(escape_sequence) @string.escape
(char_literal) @string.special

; Numbers
(integer_literal) @number
(float_literal) @number.float

; Booleans and null
(boolean_literal) @constant.builtin
(null_literal) @constant.builtin

; Types
(primitive_type) @type.builtin
(type_identifier) @type

; Keywords
[
  "fn" "val" "var" "return" "if" "else" "while" "for" "in"
  "switch" "break" "continue" "loop" "struct" "enum" "impl"
  "trait" "pub" "use" "as" "const" "extern" "rust" "test"
  "comptime" "view"
] @keyword

; Function definitions
(function_def name: (identifier) @function)
(call_expr name: (identifier) @function.call)
(method_call_expr method: (identifier) @function.method)

; Variable bindings
(val_stmt name: (identifier) @variable)
(param name: (identifier) @variable.parameter)
(lambda_param name: (identifier) @variable.parameter)
(for_stmt var: (identifier) @variable)

; Identifiers
(identifier) @variable
