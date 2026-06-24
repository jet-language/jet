; Comments
(comment) @comment
(doc_comment) @comment.doc

; Strings and chars
(string_literal) @string
(char_literal) @string.special

; Numbers
(number) @number

; Keywords — the grammar-repo grammar's `keyword` node covers the full
; JET_KEYWORD_LIST (see grammar-repo/grammar.js).  Foreign/deprecated
; spellings (val, var, while, for, switch, test, and, or, not) are absent
; from that node intentionally and will NOT be highlighted.
(keyword) @keyword

; User-defined types (PascalCase names not classified as keywords)
(type_name) @type

; All other identifiers
(identifier) @variable
