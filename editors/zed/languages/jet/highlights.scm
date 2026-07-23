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

; Source / dependency refs (`c@system`, `tool@github`)
(source_ref (ref_target) @string.special)

; Types
(primitive_type) @type.builtin
(type_identifier) @type
(generic_type base: (type_identifier) @type)
(capability_sigil) @operator

; Applied rules (#Test, #[Codable], #Caps(...), #Grant, ...)
(attribute (marker_name) @attribute)
(attribute_list (marker_name) @attribute)
(lower_marker_name) @attribute
(effect_arrow (effect_path (type_identifier) @attribute))
(marker_decl (attribute (marker_name) @attribute))

; Loop labels (@outer)
(loop_label) @label
(next_stmt "next" @keyword.control)

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

; BEGIN GENERATED JET SYNTAX HIGHLIGHTS
; keyword.control: break defer else if loop return
[
  "break"
  "defer"
  "else"
  "if"
  "loop"
  "return"
] @keyword.control

; keyword.declaration: Bench Context Impure Pure Reactive Sanitizer State Tainted Test Todo Transact Transition Unsafe add alias as change client comptime const derive distinct enum extern fn impl migration module priv protocol pub remove rename rust server state struct tag taskgroup trait use validate via
[
  "Bench"
  "Test"
  "add"
  "as"
  "change"
  "comptime"
  "const"
  "derive"
  "distinct"
  "enum"
  "extern"
  "fn"
  "impl"
  "migration"
  "module"
  "pub"
  "remove"
  "rename"
  "rust"
  "struct"
  "tag"
  "trait"
  "use"
  "via"
] @keyword

; keyword.ownership: uninit
; keyword.other: it self
[
  "self"
] @keyword

; literal: None Val false true
[
  "false"
  "true"
] @constant.builtin

; type.builtin: BTreeMap BigInt BitSet Bool Budget BudgetApplies ByteBuffer Char Computed Csv DataTree DbValue Decimal Deque Derived Effect Error Event EventPolicy EventScope EventTrace F32 F64 Float HashMap Hook I16 I32 I64 I8 IOError Int JSON JSONError Json Key Lru Measurement PriorityQueue Ptr SelectBuilder Set Shared Signal SortedSet Stream String Subscription TaskGroup Toml U16 U32 U64 U8 UTF8Error Void WatchEvent WatchHandle WatchSet Yaml
[
  "Bool"
  "Char"
  "Error"
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
  "Void"
] @type.builtin

; builtin: check input print
; marker.rule: Abi Add Bench Bindgen Caps Cli Codable CodableAsBase Comparable Context DebugOnly Decode Default DenyUnknownFields Doc Encode Every Extern FFI Flatten Grant Html Impure Inline InlineAlways Invariant Layout Live Max Meta Min Mul MustUse NoPrelude Nondeterministic Numeric Off Patchable Persist Policy Post Pre Printable PubFile PublishedSchema Pure Reactive Redact Region Rename RenameAll Replayable Sanitizer Shield SingleUse Skip Sql State Summarize Tag Tainted Target Task Test Todo Track Transact Transition UnitFamily Unsafe Untagged WasmExport allow inline static
; sigil: # & ... :: := ^ ~
; operator: ! != % %= && &= * *= + ++ += - -- -= -> .. .[ .{ / /= < << <<= <= == => > >= >> >>= ? ?. ?? ^= | |= ||
; END GENERATED JET SYNTAX HIGHLIGHTS

; Config / manifest keys (pkg.jet, env.jet)
(config_key) @property

; All other identifiers
(identifier) @variable
