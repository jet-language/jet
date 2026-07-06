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

; BEGIN GENERATED JET SYNTAX HIGHLIGHTS
; keyword.control: break continue else if in loop return step
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

; keyword.declaration: Bench Context Impure Pure Reactive Sanitizer State Tainted Test Todo Transact Transition Unsafe add alias as assume_deterministic change client comptime const derive distinct enum extern fn impl live migration module policy priv protocol pub region remove rename rust server state struct tag taskgroup trait use via
[
  "Bench"
  "Context"
  "Impure"
  "Pure"
  "Reactive"
  "Sanitizer"
  "State"
  "Tainted"
  "Test"
  "Todo"
  "Transact"
  "Transition"
  "Unsafe"
  "add"
  "alias"
  "as"
  "assume_deterministic"
  "change"
  "client"
  "comptime"
  "const"
  "derive"
  "distinct"
  "enum"
  "extern"
  "fn"
  "impl"
  "live"
  "migration"
  "module"
  "policy"
  "priv"
  "protocol"
  "pub"
  "region"
  "remove"
  "rename"
  "rust"
  "server"
  "state"
  "struct"
  "tag"
  "taskgroup"
  "trait"
  "use"
  "via"
] @keyword

; keyword.ownership: copy uninit
[
  "copy"
  "uninit"
] @keyword

; keyword.other: it self
[
  "it"
  "self"
] @keyword

; literal: None Val err false ok true
[
  "None"
  "Val"
  "err"
  "false"
  "ok"
  "true"
] @constant.builtin

; type.builtin: BigInt Bool Char Computed Csv Data DbValue Decimal Derived Effect Error F32 F64 Float I16 I32 I64 I8 IOError Int JSON JSONError Json Key List Map Measurement Ptr SelectBuilder Shared Signal Stream String TaskGroup Toml U16 U32 U64 U8 UTF8Error Yaml
[
  "BigInt"
  "Bool"
  "Char"
  "Computed"
  "Csv"
  "Data"
  "DbValue"
  "Decimal"
  "Derived"
  "Effect"
  "Error"
  "F32"
  "F64"
  "Float"
  "I16"
  "I32"
  "I64"
  "I8"
  "IOError"
  "Int"
  "JSON"
  "JSONError"
  "Json"
  "Key"
  "List"
  "Map"
  "Measurement"
  "Ptr"
  "SelectBuilder"
  "Shared"
  "Signal"
  "Stream"
  "String"
  "TaskGroup"
  "Toml"
  "U16"
  "U32"
  "U64"
  "U8"
  "UTF8Error"
  "Yaml"
] @type.builtin

; builtin: input print
[
  "input"
  "print"
] @function.builtin

; marker.directive: Bench Bindgen Caller Caps Default DenyUnknownFields Extern Flatten Grant Html Impure Js Layout Reactive Rename RenameAll Sanitizer SingleUse Skip State Suppress Tag Tainted Target Test Todo Transact Transition UnitFamily Unsafe Untagged Wasm WasmExport
; marker.contract: Cli Codable CodableAsBase Comparable Debug Decode Doc Encode Experimental Hardened Inline InlineAlways MustUse Numeric Patchable Persist Post Pre Printable PublishedSchema Pure Redact Summarize Tested
; sigil: # & ... :: := @ ^
; operator: ! != % %= && &= * *= + ++ += - -- -= -> .. .[ .{ / /= < << <<= <= == => > >= >> >>= ? ?. ?? ^= | |= ||
; END GENERATED JET SYNTAX HIGHLIGHTS

; Config / manifest keys (pkg.jet, env.jet)
(config_key) @property

; All other identifiers
(identifier) @variable
