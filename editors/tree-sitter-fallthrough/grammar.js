// Tree-sitter grammar for the Fallthrough language.
//
// The grammar follows the normative text in docs/language: declarations put the
// name before the type (`x int`, not `int x`), types are written with prefix
// operators (`*T`, `&T`, `[3]T`), and every block is braced. Where the compiler
// accepts a construct the standard does not spell out yet, the grammar accepts
// it too and highlights it the same way; the grammar is for an editor, not a
// second implementation of the type checker.
//
// Keywords are taken from the compiler's own table
// (crates/fallc-lexer/src/token.rs) so that the two cannot drift apart.

module.exports = grammar({
  name: 'fallthrough',

  extras: $ => [
    /\s/,
    $.line_comment,
    $.block_comment,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    // A block is both a statement and an expression (a lambda body, a scope
    // written where a value may stand), so the two readings overlap.
    [$.statement, $.expression],
    // A name is both a type and a value; only the checker can tell them apart,
    // and the grammar accepts both readings rather than guessing.
    [$.type, $.expression],
    [$.variable_declaration_statement, $.expression_statement],
  ],

  supertypes: $ => [
    $.statement,
    $.type,
    $.expression,
  ],

  rules: {
    source_file: $ => repeat($._item),

    _expression: $ => choice(
      $.literal,
      $.parenthesized_expression,
      $.block,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.unary_expression,
      $.binary_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.ternary_expression,
      $.cast_expression,
      $.prefix_expression,
      $.postfix_expression,
      $.propagate_expression,
      $.lambda_expression,
      $.constructor_expression,
      $.designated_initializer,
      $.sizeof_expression,
      $.alignof_expression,
      $.template_call,
      $.macro_invocation,
      $.this,
      $.nullptr,
    ),

    // ------------------------------------------------------------ items

    // An attribute may precede a declaration that carries one. The declaration
    // rules below do not each repeat the attribute, so `@name template <..>`
    // is read here and nowhere twice.
    _item: $ => choice(
      seq(repeat1($.attribute), $._declaration),
      $._declaration,
    ),

    _declaration: $ => choice(
      $.import_declaration,
      $.export_declaration,
      $.function_declaration,
      $.type_declaration,
      $.union_declaration,
      $.enum_declaration,
      $.interface_declaration,
      $.traits_declaration,
      $.variable_declaration,
      $.extern_variable_declaration,
      $.link_declaration,
      $.macro_definition,
      $.template_declaration,
      $.setup_block,
      $.static_assert_statement,
    ),

    import_declaration: $ => seq(
      'import',
      $.module_path,
      ';',
    ),

    // `from M import x;`, `from M import *;`, and the exported form.
    from_import: $ => seq(
      'from',
      $.module_path,
      'import',
      choice($.import_list, '*'),
      ';',
    ),

    import_list: $ => seq(
      $.identifier,
      repeat(seq(',', $.identifier)),
    ),

    module_path: $ => prec.left(seq(
      $.identifier,
      repeat(seq('.', $.identifier)),
    )),

    export_declaration: $ => seq(
      'export',
      choice(
        $.function_declaration,
        $.type_declaration,
        $.union_declaration,
        $.enum_declaration,
        $.interface_declaration,
        $.traits_declaration,
        $.variable_declaration,
        $.extern_variable_declaration,
        $.template_declaration,
        $.macro_definition,
        $.from_import,
      ),
    ),

    template_declaration: $ => seq(
      'template',
      $.template_parameter_list,
      choice(
        $.function_declaration,
        $.type_declaration,
        $.union_declaration,
        $.enum_declaration,
        $.interface_declaration,
        $.traits_declaration,
        $.variable_declaration,
      ),
    ),

    template_parameter_list: $ => seq(
      '<',
      commaSep1($._template_parameter),
      '>',
    ),

    _template_parameter: $ => choice(
      $.type_parameter,
      $.value_parameter,
      $.pack_parameter,
    ),

    type_parameter: $ => prec(1, seq($.identifier, 'type')),

    value_parameter: $ => seq($.type, $.identifier),

    // `values ...int` (a value pack) and `Args ...type` (a type pack): the
    // name comes first, the ellipsis marks the pack, then its element type.
    pack_parameter: $ => seq(
      field('name', $.identifier),
      '...',
      choice('type', $.type),
    ),

    attribute: $ => seq('@', $.identifier, optional($.argument_list)),

    function_declaration: $ => seq(
      'fn',
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(field('return_type', $.type)),
      choice(
        field('body', $.block),
        ';',
        seq('except', optional(field('throws_condition', $.parenthesized_expression)), choice($.block, ';')),
      ),
    ),

    parameter_list: $ => seq(
      '(',
      optional(commaSep1($.parameter)),
      ')',
    ),

    parameter: $ => seq(
      field('name', $.identifier),
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
    ),

    type_declaration: $ => seq(
      'type',
      field('name', $.identifier),
      '{',
      repeat(choice($.field_declaration, $._item)),
      '}',
    ),

    union_declaration: $ => seq(
      'union',
      field('name', $.identifier),
      '{',
      repeat($.field_declaration),
      '}',
    ),

    // A struct is a `type` whose body is fields.
    field_declaration: $ => prec(5, seq(
      field('name', $.identifier),
      field('type', $.type),
      optional(seq('=', field('default', $.expression))),
      ';',
    )),

    enum_declaration: $ => seq(
      'enum',
      field('name', $.identifier),
      field('base', $.type),
      '{',
      optional(commaSep1($.enumerator)),
      optional(','),
      '}',
    ),

    enumerator: $ => seq(
      field('name', $.identifier),
      optional(seq('=', field('value', $.expression))),
    ),

    interface_declaration: $ => seq(
      'interface',
      field('name', $.identifier),
      '{',
      repeat(seq(choice($.function_declaration, $.data_member), optional(';'))),
      '}',
    ),

    // A member of an `interface` or a `traits` body has no receiver rule of
    // its own; it reads like a function declaration.
    data_member: $ => seq($.identifier, $.type),

    traits_declaration: $ => seq(
      'traits',
      field('name', $.identifier),
      '{',
      repeat(choice($.function_declaration, $.data_member)),
      '}',
    ),

    variable_declaration: $ => seq(
      field('name', $.identifier),
      field('type', $.type),
      optional(seq('=', field('value', $.expression))),
      ';',
    ),

    extern_variable_declaration: $ => seq(
      'extern',
      choice(
        seq('"', $.string_content, '"'),
        $.identifier,
      ),
      field('name', $.identifier),
      field('type', $.type),
      ';',
    ),

    link_declaration: $ => seq(
      'link',
      $.identifier,
      choice('static', 'dynamic'),
      'from',
      $.string_literal,
      ';',
    ),

    // `define NAME body`, and the exported spelling `export define`.
    macro_definition: $ => seq(
      'define',
      field('name', $.identifier),
      optional($.macro_parameters),
      field('body', choice($.block, $._macro_body)),
    ),

    macro_parameters: $ => seq('(', commaSep($.identifier), ')'),

    _macro_body: $ => /[^\n]*/,

    setup_block: $ => seq('__setup__', $.block),

    // ------------------------------------------------------------ statements

    statement: $ => choice(
      $.variable_declaration_statement,
      $.block,
      $.expression_statement,
      $.if_statement,
      $.while_statement,
      $.for_statement,
      $.range_for_statement,
      $.switch_statement,
      $.return_statement,
      $.break_statement,
      $.continue_statement,
      $.fallthrough_statement,
      $.raise_statement,
      $.try_statement,
      $.defer_statement,
      $.static_assert_statement,
      $.empty_statement,
    ),

    block: $ => seq('{', repeat($._statement_or_declaration), '}'),

    _statement_or_declaration: $ => choice($.statement, $.function_declaration, $.type_declaration),

    // A binding puts its name before its type, which reads like two
    // identifiers in a row. In a block that is a declaration, not an
    // expression, so the declaration wins the tie.
    variable_declaration_statement: $ => prec.dynamic(20, prec(20, seq(
      optional('static'),
      field('name', $.identifier),
      field('type', $.type),
      optional(seq('=', field('value', $.expression))),
      ';',
    ))),

    // A statement that is a bare identifier has no effect, and allowing one is
    // what made `s String = ..` ambiguous with two expressions. Assignment,
    // calls and every other statement still start with an expression.
    expression_statement: $ => prec(-1, seq($.expression_without_identifier, ';')),

    // The same expression grammar without a bare name, for statement position:
    // a statement that is only a name does nothing, and allowing one is what
    // made `s String = ..` ambiguous with two expressions in a row.
    expression_without_identifier: $ => choice(
      $.literal,
      $.parenthesized_expression,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.unary_expression,
      $.binary_expression,
      $.ternary_expression,
      $.cast_expression,
      $.prefix_expression,
      $.postfix_expression,
      $.propagate_expression,
      $.lambda_expression,
      $.designated_initializer,
      $.sizeof_expression,
      $.alignof_expression,
      $.template_call,
      $.macro_invocation,
    ),

    empty_statement: $ => ';',

    if_statement: $ => seq(
      'if',
      field('condition', $.parenthesized_expression),
      optional($.attribute),
      field('consequence', $.block),
      optional(seq('else', optional($.attribute), field('alternative', choice($.block, $.if_statement)))),
    ),

    while_statement: $ => seq(
      'while',
      field('condition', $.parenthesized_expression),
      field('body', $.block),
      optional(seq('else', field('alternative', $.block))),
    ),

    for_statement: $ => seq(
      'for',
      '(',
      optional(choice($.variable_declaration_statement_in_for, $.expression)),
      optional(seq(';', optional(field('condition', $.expression)))),
      ';',
      optional(field('update', $.expression)),
      ')',
      field('body', $.block),
      optional(seq('else', field('alternative', $.block))),
    ),

    variable_declaration_statement_in_for: $ => prec(5, seq(
      field('name', $.identifier),
      field('type', $.type),
      optional(seq('=', field('value', $.expression))),
    )),

    range_for_statement: $ => prec(5, seq(
      'for',
      '(',
      field('name', $.identifier),
      field('type', $.type),
      ':',
      field('iterable', $.expression),
      ')',
      field('body', $.block),
      optional(seq('else', field('alternative', $.block))),
    )),

    switch_statement: $ => seq(
      'switch',
      field('value', $.parenthesized_expression),
      '{',
      repeat($.case_clause),
      optional($.default_clause),
      '}',
    ),

    case_clause: $ => seq(
      'case',
      commaSep1($.expression),
      optional($.attribute),
      ':',
      repeat($._statement_or_declaration),
    ),

    default_clause: $ => seq(
      'default',
      ':',
      repeat($._statement_or_declaration),
    ),

    return_statement: $ => seq('return', optional(field('value', $.expression)), ';'),

    break_statement: $ => seq('break', ';'),

    continue_statement: $ => seq('continue', ';'),

    fallthrough_statement: $ => seq('fallthrough', ';'),

    raise_statement: $ => seq(
      'raise',
      optional(seq(field('type', $.string_literal), optional(seq(':', field('message', $.expression))))),
      ';',
    ),

    static_assert_statement: $ => seq(
      'static_assert',
      '(',
      field('condition', $.expression),
      optional(seq(',', field('message', $.string_literal))),
      ')',
      ';',
    ),

    // `try { .. } except (..) { .. } except { .. } else { .. } finally { .. }`
    try_statement: $ => prec.right(seq(
      'try',
      field('body', $.block),
      repeat($.except_clause),
      optional(seq('else', field('else_body', $.block))),
      optional(seq('finally', field('finally_body', $.block))),
    )),

    except_clause: $ => seq(
      'except',
      optional(seq('(', commaSep1($.string_literal), ')')),
      field('body', $.block),
    ),

    defer_statement: $ => seq('defer', field('body', $.block)),

    // ------------------------------------------------------------ types

    // `x int` puts the name before the type, so a declaration opens with two
    // identifiers in a row. The type parser is preferred there, which is what
    // keeps `s String = ..` a declaration instead of two expressions.
    type: $ => prec.dynamic(2, choice(
      $._simple_type,
      $.pointer_type,
      $.const_pointer_type,
      $.reference_type,
      $.const_reference_type,
      $.rvalue_reference_type,
      $.const_rvalue_reference_type,
      $.array_type,
      $.function_type,
      $.const_type,
      $.volatile_type,
      $.auto_type,
    )),

    _simple_type: $ => choice(
      $.primitive_type,
      $.generic_type,
      // A declared type: `String`, `MyStruct`, `Parser`. Nothing in the
      // grammar can tell a type name from a value name, and the checker is
      // what does; a syntax tree only has to accept the shape. It is a
      // separate name so a highlight query can colour a type position as a
      // type without colouring every name.
      alias($.identifier, $.type_identifier),
    ),

    primitive_type: _ => choice(
      // Integers, with their widths.
      'i8', 'i16', 'i32', 'i64', 'i128', 'isize',
      'u8', 'u16', 'u32', 'u64', 'u128', 'usize',
      'int', 'uint', 'byte',
      // Floating point.
      'f32', 'f64', 'float', 'double',
      // Characters, from the narrow to the wide.
      'c8', 'c16', 'c32', 'char',
      // Named types the language always has.
      'bool', 'void',
    ),

    generic_type: $ => prec(1, seq(
      field('name', $.identifier),
      '<',
      commaSep1($._type_argument),
      '>',
    )),


    // A parenthesised type is written `(T)` and reads exactly like a
    // parenthesised expression; the grammar takes the expression reading,
    // which is also what a highlight should colour, and the checker decides
    // what a parenthesised name means in a type position.
    paren_type: $ => prec.dynamic(-1, seq('(', $.type, ')')),

    pointer_type: $ => prec(2, seq('*', $.type)),
    const_pointer_type: $ => prec(3, seq('*', 'const', $.type)),
    reference_type: $ => prec(2, seq('&', $.type)),
    const_reference_type: $ => prec(3, seq('&', 'const', $.type)),
    rvalue_reference_type: $ => prec(2, seq('&&', $.type)),
    const_rvalue_reference_type: $ => prec(3, seq('&&', 'const', $.type)),
    array_type: $ => prec(1, seq('[', optional($.expression), ']', $.type)),
    const_type: $ => prec(1, seq('const', $.type)),
    volatile_type: $ => prec(1, seq('volatile', $.type)),
    auto_type: _ => 'auto',

    function_type: $ => prec.right(seq(
      'fn',
      optional(seq('"', $.string_content, '"')),
      $.parameter_list,
      optional($.type),
    )),

    // ------------------------------------------------------------ expressions

    expression: $ => choice(
      $.identifier,
      $.literal,
      $.parenthesized_expression,
      $.block,
      $.call_expression,
      $.method_call_expression,
      $.field_expression,
      $.index_expression,
      $.unary_expression,
      $.binary_expression,
      $.assignment_expression,
      $.compound_assignment_expression,
      $.ternary_expression,
      $.cast_expression,
      $.postfix_expression,
      $.prefix_expression,
      $.propagate_expression,
      $.lambda_expression,
      $.constructor_expression,
      $.designated_initializer,
      $.sizeof_expression,
      $.alignof_expression,
      $.type_query,
      $.throw_query,
      $.template_call,
      $.macro_invocation,
      $.this,
      $.nullptr,
      $.boolean_literal,
    ),

    parenthesized_expression: $ => seq('(', choice($._expression_or_comma_list, $.fold_expression), ')'),

    // `(values + ...)`: a pack and an operator, folded at compile time.
    fold_expression: _ => token(seq(
      /[\p{XID_Start}_][\p{XID_Continue}_]*/,
      /\s*/,
      /[+\-*\/%&|^]=?|&&|\|\||<<|>>|==|!=|<=|>=|<|>/,
      /\s*/,
      '\.\.\.',
    )),

    _expression_or_comma_list: $ => seq($.expression, optional(seq(',', commaSep1($.expression)))),

    call_expression: $ => prec(15, seq(
      field('function', $.expression),
      field('arguments', $.argument_list),
    )),

    template_call: $ => prec.right(16, seq(
      field('function', $.identifier),
      field('arguments', $.template_argument_list),
      optional($.argument_list),
    )),

    template_argument_list: $ => seq(
      '<',
      commaSep1($._type_argument),
      '>',
    ),

    // What may stand between `<` and `>`: a type, or a value the compiler
    // knows at instantiation time. Keeping the set small is what lets the
    // grammar tell `a < b` from `a<T>` without a semantic pass.
    _type_argument: $ => choice(
      $.type,
      $.integer_literal,
      $.boolean_literal,
      $.string_literal,
      $.qualified_name,
    ),

    // `util.move` and `C.printf`: a dotted name in expression position is a
    // field of a module, and a template argument may be one.
    qualified_name: $ => prec.right(seq($.identifier, '.', $.identifier)),

    // A method call with explicit template arguments outranks a field access,
    // so `a.b<T>(..)` is a call and `a.b` stays a field.
    method_call_expression: $ => prec(17, seq(
      field('receiver', $.expression),
      choice('.', '->'),
      field('method', $.identifier),
      optional($.template_argument_list),
      field('arguments', $.argument_list),
    )),

    argument_list: $ => seq(
      '(',
      optional(seq(commaSep1($.expression), optional('...'))),
      ')',
    ),

    field_expression: $ => prec(15, seq(
      field('value', $.expression),
      choice('.', '->'),
      field('field', $.identifier),
    )),

    index_expression: $ => prec(15, seq(
      field('value', $.expression),
      '[',
      field('index', $.expression),
      ']',
    )),

    unary_expression: $ => prec(14, seq(
      field('operator', choice('-', '+', '!', '~', '*', '&')),
      field('operand', $.expression),
    )),

    prefix_expression: $ => prec(14, seq(
      field('operator', choice('++', '--')),
      field('operand', $.expression),
    )),

    postfix_expression: $ => prec(16, seq(
      field('operand', $.expression),
      field('operator', choice('++', '--')),
    )),

    propagate_expression: $ => prec(16, seq(
      field('value', $.expression),
      '?!',
    )),

    binary_expression: $ => {
      const table = [
        ['||', 1],
        ['&&', 2],
        ['|', 3],
        ['^', 4],
        ['&', 5],
        ['==', 6],
        ['!=', 6],
        ['<', 7],
        ['>', 7],
        ['<=', 7],
        ['>=', 7],
        ['<=>', 7],
        ['<<', 8],
        ['>>', 8],
        ['+', 9],
        ['-', 9],
        ['*', 10],
        ['/', 10],
        ['%', 10],
        ['**', 11],
      ];
      return choice(...table.map(([operator, precedence]) => prec.left(precedence, seq(
        field('left', $.expression),
        field('operator', operator),
        field('right', $.expression),
      ))));
    },

    assignment_expression: $ => prec.right(0, seq(
      field('left', $.expression),
      '=',
      field('right', $.expression),
    )),

    compound_assignment_expression: $ => prec.right(0, seq(
      field('left', $.expression),
      field('operator', choice('+=', '-=', '*=', '/=', '%=', '&=', '|=', '^=', '<<=', '>>=', '**=')),
      field('right', $.expression),
    )),

    ternary_expression: $ => prec.right(1, seq(
      field('condition', $.expression),
      '?',
      field('consequence', $.expression),
      ':',
      field('alternative', $.expression),
    )),

    cast_expression: $ => prec(12, seq(
      field('value', $.expression),
      'as',
      field('type', $.type),
    )),

    // The `-> expr` body takes everything to the right, so the lambda binds
    // looser than any operator in it.
    lambda_expression: $ => prec.right(20, seq(
      'lambda',
      optional($.parameter_list),
      choice($.block, seq('->', $.expression)),
    )),

    // `T{ .. }` and `.{ .. }`, with either positional or designated elements.
    constructor_expression: $ => prec(13, seq(
      optional(field('type', $.type)),
      '{',
      optional(commaSep1($.expression)),
      '}',
    )),

    designated_initializer: $ => prec(13, seq(
      '.{',
      optional(commaSep1(choice(
        $.designated_element,
        $.expression,
      ))),
      '}',
    )),

    designated_element: $ => prec(6, seq(
      field('field', $.identifier),
      '=',
      field('value', $.expression),
    )),

    // `sizeof(T)` and `sizeof(..)`: the argument is a type, which is written
    // exactly like an expression until the checker says otherwise.
    sizeof_expression: $ => prec.dynamic(1, prec(14, seq(
      'sizeof',
      choice(
        seq('(', $.type, ')'),
        seq('(', '...', '(', $.identifier, ')', ')'),
      ),
    ))),

    alignof_expression: $ => prec(14, seq('alignof', '(', $.type, ')')),

    type_query: $ => seq('type', '(', $.expression, ')'),

    throw_query: $ => seq('except', '(', $.expression, ')'),

    macro_invocation: $ => prec.right(16, seq('$', $.identifier, optional($.argument_list))),

    // ------------------------------------------------------------ literals

    literal: $ => choice(
      $.string_literal,
      $.character_literal,
      $.integer_literal,
      $.float_literal,
    ),

    string_literal: $ => choice(
      $.raw_string_literal,
      $.multiline_string_literal,
      $.string,
    ),

    string: $ => seq(
      optional(choice('u16', 'u32', 'b')),
      '"',
      repeat(choice($.escape_sequence, $.string_content)),
      '"',
    ),

    multiline_string_literal: $ => seq(
      optional(choice('u16', 'u32')),
      '"""',
      repeat(choice($.escape_sequence, $.string_content)),
      '"""',
    ),

    raw_string_literal: _ => token(seq(
      'r',
      optional(/#+"|"/),
      /[^"]*/,
      '"',
      optional(/#+/),
    )),

    escape_sequence: _ => token(prec(1, seq(
      '\\',
      choice(
        /[\\'"0nrtbfv]/,
        /x[0-9a-fA-F]{2}/,
        /u\{[0-9a-fA-F]+\}/,
      ),
    ))),

    string_content: _ => token(prec(1, /[^"\\\n]+/)),

    character_literal: $ => seq(
      optional(choice('u16', 'u32')),
      "'",
      choice($.escape_sequence, /[^'\\\n]/),
      "'",
    ),

    integer_literal: _ => token(choice(
      /0[xX][0-9a-fA-F_]+(i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)?/,
      /0[oO][0-7_]+(i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)?/,
      /0[bB][01_]+(i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)?/,
      /[0-9][0-9_]*(i8|i16|i32|i64|i128|isize|u8|u16|u32|u64|u128|usize)?/,
    )),

    float_literal: _ => token(choice(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?f?/,
      /[0-9][0-9_]*[eE][+-]?[0-9]+f?/,
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?/,
    )),

    boolean_literal: _ => choice('true', 'false'),

    this: _ => 'this',

    nullptr: _ => 'nullptr',

    // ------------------------------------------------------------ the end

    identifier: _ => /[_\p{XID_Start}][_\p{XID_Continue}]*/,

    line_comment: _ => token(seq('//', /[^\n]*/)),

    block_comment: _ => token(seq('/*', /[^*]*\*+([^/*][^*]*\*+)*/, '/')),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(',', rule)));
}
