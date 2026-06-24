; Jet syntax highlighting.
; Node names here MUST match the compiled grammar (grammar-repo/grammar.js →
; src/node-types.json). A reference to a node the grammar doesn't define makes
; the WHOLE language fail to load in Zed ("Invalid node type ..."), so keep this
; query in sync with the grammar whenever keyword/literal nodes change.

; Comments
(comment) @comment
(doc_comment) @comment.doc

; Literals
(string_literal) @string
(string_interpolation) @string
(escape_sequence) @string.escape
(char_literal) @string.special
(integer_literal) @number
(float_literal) @number
(boolean_literal) @boolean
(null_literal) @constant.builtin

; Types
(primitive_type) @type.builtin
(type_identifier) @type

; Keywords (the grammar exposes each keyword as its own anonymous token —
; there is no single `keyword` node). `true`/`false` are `boolean_literal`.
[
  "as"
  "break"
  "comptime"
  "const"
  "continue"
  "else"
  "enum"
  "extern"
  "fn"
  "for"
  "if"
  "impl"
  "in"
  "loop"
  "mut"
  "pub"
  "ref"
  "return"
  "rust"
  "struct"
  "switch"
  "take"
  "test"
  "trait"
  "use"
  "val"
  "var"
  "view"
  "while"
] @keyword

; Operators
[
  "+" "-" "*" "/" "%"
  "=" "==" "!=" "<" ">" "<=" ">="
  "&&" "||" "!"
  "&" "|" "^" "<<" ">>"
  "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "->" "=>" ".." "?"
] @operator

; All other identifiers
(identifier) @variable
