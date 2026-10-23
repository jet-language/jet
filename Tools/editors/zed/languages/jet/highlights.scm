; Jet syntax highlighting for Zed.
; Node names here MUST exist in the compiled grammar (Tools/editors/tree-sitter/
; grammar.js → src/node-types.json): a query naming a missing node or a string
; that is not an anonymous grammar token makes Zed drop the WHOLE language
; ("Invalid node type ..."). String literals in this file are therefore only
; anonymous grammar tokens; contextual words are colored through the named
; `keyword`, `control_keyword` and `decl_head` nodes instead of predicates.
; When several patterns capture the same node, the later pattern wins in Zed,
; so general captures come first and positional refinements follow.

; Comments
(line_comment) @comment
(block_comment) @comment
(doc_comment) @comment.doc

; Literals
(string_literal) @string
(multiline_string) @string
(raw_string) @string
(escape_sequence) @string.escape
(char_literal) @string.special
(number_literal) @number
(boolean) @boolean

; Types
(type_identifier) @type
(primitive_type) @type.builtin

; Applied rules (#Test, #Transition(A, B), #[Codable], @allow) and $facts
(attribute) @attribute
(lower_attribute) @attribute
(meta_name) @variable.special

; Keywords outside the generated anonymous set below
(keyword) @keyword
(control_keyword) @keyword.control

; D-ECO-INLINEPACKAGE1=A: the contextual carrier word is highlighted only
; inside the structural inline_package node, so `priv(package)` remains ordinary
; module-visibility syntax everywhere else.
(inline_package "package" @keyword)

; BEGIN GENERATED JET SYNTAX HIGHLIGHTS
; keyword.control: after break defer else if in loop next return task task.all task.any task.group task.race yield
[
  "break"
  "else"
  "if"
  "in"
  "loop"
  "return"
] @keyword.control

; keyword.declaration: Context Impure Reactive Scrub State Test Todo Transact Transition Unsafe add alias as change client derive distinct effect enum extern fact fn impl marker migration module policy priv protocol pub remove rename rust server state struct tag trait use validate via wrap
[
  "derive"
  "enum"
  "extern"
  "fn"
  "impl"
  "module"
  "pub"
  "struct"
  "tag"
  "trait"
  "use"
] @keyword

; keyword.ownership: uninit
; keyword.other: $build $package $phase $program @SOURCE @TYPES comptime it self shared
[
  "comptime"
  "self"
] @keyword

; literal: Cancelled DeadlineBlown None Panicked Val false true
[
  "false"
  "true"
] @constant.builtin

; type.builtin: () BTreeMap Bits Bool Budget BudgetApplies Bytes CSV Cache Char Complex Computed Condition DBValue Decimal Derived Duration Dyn Effect EncodingError Err Event EventPolicy EventScope EventTrace F32 F64 Float HashMap Hook I16 I32 I64 I8 IOError Instant Int Iter JSON Key Measurement MemoStats PriorityQueue Ptr Queue Rank Receiver Sender Set Shared Shared.Weak SharedGuard SharedRevisionError SharedSnapshot Signal Stream String Subscription TOML Task TaskFailure U16 U32 U64 U8 UTF8Error ViewIter WatchEvent WatchHandle WatchSet YAML
[
  "Bool"
  "Char"
  "Err"
  "F32"
  "F64"
  "Float"
  "I16"
  "I32"
  "I64"
  "I8"
  "Int"
  "String"
  "U16"
  "U32"
  "U64"
  "U8"
] @type.builtin

; builtin: assert assert_eq channel check freeze input join print
; marker.rule: ABI Arithmetic Bindgen CLI Close Codable CodableAsBase Commutative Comparable Context Debug DebugOnly Decode DenyUnknownFields Deprecated DevPanel Discriminant Doc Encode Env Equatable Error Every FFI FX Flag Flatten HTML Impure Inline Interrupt Job Kernel Layout Live Local Memo Meta MustUse NoPrelude Nondeterministic Numeric Off Patchable Persist Policy Post Pre Printable PublishedSchema Reactive Receipt Redact Region Rename RenameAll Replayable Root SQL Scalar Scrub Shared Shield Short SingleUse Skip State Static Target Test Todo Track Transact Transition Undo UnitFamily Unsafe Untagged WasmExport allow wire
; sigil: # $ & ... :: := :> <: ^
; operator: ! != % %% %%= %= && * *= + += - -= -> .. ..< .[ / /% /%= /= < << <<= <= <=> == > >= >> >>= ? ?. ?? | ||
; END GENERATED JET SYNTAX HIGHLIGHTS

; Operators and punctuation
[
  "!" "!=" "%" "%%" "%%=" "%=" "&" "&&" "&=" "*" "*=" "+" "+=" "-" "-=" "->"
  ".." "..." "..<" "/" "/%" "/%=" "/=" "::" ":=" ":>" "<" "<:" "<<" "<<="
  "<=" "<=>" "=" "==" "=>" ">" ">=" ">>" ">>=" "?" "?." "??" "^" "^=" "|"
  "|=" "||" "~" "~|" "~|=" "#" "$" "@"
] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket

["," "." ":"] @punctuation.delimiter

; Delimiters that belong to a construct (after the generic punctuation above)
(effect_row
  "-[" @operator
  "]>" @operator)
(list_import ".[" @punctuation.bracket)
(attribute_list
  "#[" @attribute
  "]" @attribute)

; `{expr}` inside a string is live Jet: the braces are delimiters and the
; expression keeps its own colors.
(interpolation
  "{" @punctuation.special
  "}" @punctuation.special) @embedded

; Contextual declaration words directly before a type name
; (`state Pending`, `protocol Checkout`, `marker Pattern`, `alias Id`).
(decl_head keyword: (identifier) @keyword)

; Definitions
(function_def name: (identifier) @function)
(function_def name: (type_identifier) @function)
(function_def name: (meta_name) @function)
(binding name: (type_identifier) @constant)

; Calls, members, constructors and variants
(call_expr function: (identifier) @function.call)
(method_call_expr method: (identifier) @function.method)
(member_expr field: (identifier) @property)
(constructor type: (type_identifier) @constructor)
(variant name: (type_identifier) @variant)

; Fields, record keys, named arguments, parameters
(label name: (identifier) @property)
(parameters (label name: (identifier) @variable.parameter))

; Result handler payload names (`value ? ok -> … ! err -> …`)
(result_handler_expr ok: (identifier) @variable.parameter)
(result_handler_expr error: (identifier) @variable.parameter)

