; Scopes for config.toml `not_in` bracket rules: no auto-closed quotes inside
; text or comments.
[
  (string_literal)
  (multiline_string)
  (raw_string)
] @string

[
  (line_comment)
  (block_comment)
  (doc_comment)
] @comment
