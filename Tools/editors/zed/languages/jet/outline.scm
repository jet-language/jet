(function_def
  name: (_) @name
  "fn" @context) @item

(function_def
  "fn" @context
  name: (_) @name) @item

(struct_def
  name: (_) @name
  "struct" @context) @item

(enum_def
  name: (_) @name
  "enum" @context) @item

(trait_def
  name: (_) @name
  "trait" @context) @item

(tag_def
  name: (_) @name
  "tag" @context) @item

(impl_block
  "impl" @context
  type: (_) @name) @item

(module_def
  name: (_) @name
  "module" @context) @item

(decl_head
  keyword: (identifier) @context
  name: (type_identifier) @name) @item
