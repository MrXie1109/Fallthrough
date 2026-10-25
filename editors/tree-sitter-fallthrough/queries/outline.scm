(function_declaration
  "fn" @context
  name: (identifier) @name) @item

(type_declaration
  "type" @context
  name: (identifier) @name) @item

(union_declaration
  "union" @context
  name: (identifier) @name) @item

(enum_declaration
  "enum" @context
  name: (identifier) @name) @item

(interface_declaration
  "interface" @context
  name: (identifier) @name) @item

(traits_declaration
  "traits" @context
  name: (identifier) @name) @item

(template_declaration
  "template" @context
  (function_declaration
    "fn" @context
    name: (identifier) @name) @item)

(template_declaration
  "template" @context
  (type_declaration
    "type" @context
    name: (identifier) @name) @item)
