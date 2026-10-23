; Syntax highlighting for Fallthrough.
;
; The captures are the ones Zed's themes define. Where a name is a type in one
; position and a value in another, the grammar names the type position
; separately (`type_identifier`), so a type is coloured as a type.

; ---------------------------------------------------------------- comments

(line_comment) @comment
(block_comment) @comment

; ---------------------------------------------------------------- keywords

[
  "import" "from" "export" "link" "define"
  "fn" "type" "union" "enum" "interface" "traits" "template"
  "if" "else" "while" "for" "switch" "case" "default"
  "break" "continue" "return" "fallthrough" "defer"
  "raise" "try" "except" "finally"
  "lambda"
  "const" "static" "volatile" "as"
  "sizeof" "alignof" "static_assert" "extern" "dynamic"
] @keyword

(auto_type) @keyword

(import_declaration "import" @keyword.import)
(from_import "from" @keyword.import)

(attribute "@" @punctuation.special)
(attribute (identifier) @attribute)

; ---------------------------------------------------------------- types

(primitive_type) @type.builtin
(type_identifier) @type
(generic_type (identifier) @type)
(generic_type "<" @punctuation.bracket)
(generic_type ">" @punctuation.bracket)

(pointer_type "*" @operator)
(const_pointer_type "const" @keyword)
(reference_type "&" @operator)
(const_reference_type "const" @keyword)
(rvalue_reference_type "&&" @operator)
(array_type "[" @punctuation.bracket)
(array_type "]" @punctuation.bracket)

(function_type "fn" @keyword.function)

; ---------------------------------------------------------------- functions

(function_declaration name: (identifier) @function)
(method_call_expression method: (identifier) @function.method)
(call_expression function: (identifier) @function.call)
(template_call function: (identifier) @function.call)
(macro_invocation "$" @punctuation.special)
(macro_invocation (identifier) @function.macro)
(macro_definition name: (identifier) @function.macro)

(parameter name: (identifier) @variable.parameter)

; ---------------------------------------------------------------- variables

(variable_declaration name: (identifier) @variable)
(variable_declaration_statement name: (identifier) @variable)
(variable_declaration_statement_in_for name: (identifier) @variable)
(range_for_statement name: (identifier) @variable)
(field_declaration name: (identifier) @property)
(designated_element field: (identifier) @property)
(field_expression field: (identifier) @property)


(enumerator name: (identifier) @constant)
(interface_declaration name: (identifier) @type)
(traits_declaration name: (identifier) @type)
(type_declaration name: (identifier) @type)
(union_declaration name: (identifier) @type)
(enum_declaration name: (identifier) @type)
(type_parameter (identifier) @type)
(pack_parameter name: (identifier) @variable.parameter)

(except_clause (string_literal) @string.special)
(raise_statement type: (string_literal) @string.special)

; ---------------------------------------------------------------- literals

(string) @string
(string_content) @string
(multiline_string_literal) @string
(raw_string_literal) @string
(escape_sequence) @string.escape

(character_literal) @character

(integer_literal) @number
(float_literal) @number.float
(boolean_literal) @boolean

; ---------------------------------------------------------------- operators

[
  "+" "-" "*" "/" "%" "**"
  "&" "|" "^" "<<" ">>"
  "==" "!=" "<" ">" "<=" ">=" "<=>"
  "&&" "||" "!"
  "=" "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
  "++" "--"
  "?" "?!" "->"
] @operator

(cast_expression "as" @keyword.operator)
(propagate_expression "?!" @operator)

; ---------------------------------------------------------------- punctuation

["(" ")" "{" "}" "[" "]"] @punctuation.bracket
["," ";" "." ":" "..." "<" ">"] @punctuation.delimiter
