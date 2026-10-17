(function_def
  "fn" @context
  name: (_) @name) @item

(struct_def
  "struct" @context
  name: (_) @name) @item

(enum_def
  "enum" @context
  name: (_) @name) @item

(trait_def
  "trait" @context
  name: (_) @name) @item

(tag_def
  "tag" @context
  name: (_) @name) @item

(impl_block
  "impl" @context
  type: (_) @name) @item

(module_def
  "module" @context
  name: (_) @name) @item

(decl_head
  keyword: (identifier) @context
  name: (type_identifier) @name) @item
