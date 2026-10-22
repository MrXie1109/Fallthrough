//! Behaviour of the C-style macro preprocessor.
//!
//! Every test lexes real source, runs [`expand_macros`], and compares the
//! resulting tokens' spellings (with token kinds checked where the difference
//! matters, such as a pasted number). The expected spellings are written the
//! way the programmer wrote them, so the tests also pin that expansion does not
//! invent or drop spelling: `"a + b"` below is a string literal whose contents
//! are `a + b`, with the quotes belonging to the token.

use fallc_diag::DiagnosticCode;
use fallc_lexer::{Lexer, MacroDiagnostic, Punctuation, Token, TokenKind, expand_macros};
use fallc_span::{FileId, Span};

/// Lexes `source` into a token stream.
fn lex(source: &str) -> Vec<Token> {
    let mut lexer = Lexer::default();
    lexer.tokenize(source, FileId::from_raw(0)).tokens
}

/// Expands `source`, panicking if the preprocessor reports anything.
fn expand(source: &str) -> Vec<Token> {
    expand_macros(lex(source)).unwrap_or_else(|diagnostics| {
        panic!("unexpected macro diagnostics: {diagnostics:?}");
    })
}

/// Expands `source` and returns the output tokens' spellings, without `Eof`.
fn spellings(source: &str) -> Vec<String> {
    tokens_to_spellings(&expand(source))
}

/// Renders a token's spelling; a token with none is rendered by its kind.
fn spelling(token: &Token) -> String {
    token
        .spelling()
        .map_or_else(|| format!("{:?}", token.kind), ToOwned::to_owned)
}

/// Renders a token stream's spellings, without `Eof`.
fn tokens_to_spellings(tokens: &[Token]) -> Vec<String> {
    tokens
        .iter()
        .filter(|token| !token.is_eof())
        .map(spelling)
        .collect()
}

/// Expands `source` and returns the diagnostics, panicking on success.
fn diagnostics(source: &str) -> Vec<MacroDiagnostic> {
    match expand_macros(lex(source)) {
        Ok(tokens) => panic!("expected an error, got: {:?}", tokens_to_spellings(&tokens)),
        Err(diagnostics) => diagnostics,
    }
}

/// Returns the printable diagnostic codes reported for `source`.
///
/// `MacroDiagnostic::code` is a lexer-stage number; the driver renders it with
/// `DiagnosticCode::lexer`, which is what these tests check.
fn codes(source: &str) -> Vec<String> {
    diagnostics(source)
        .iter()
        .map(|diagnostic| DiagnosticCode::lexer(diagnostic.code).to_string())
        .collect()
}

/// Returns the span of the `$NAME` invocation beginning at the first `$`.
fn invocation_span(source: &str) -> Span {
    let tokens = lex(source);
    // The last `$` is the top-level invocation; an earlier one may be inside a
    // definition's body.
    let dollar = tokens
        .iter()
        .rposition(|token| token.kind == TokenKind::Punct(Punctuation::Dollar))
        .expect("the source has an invocation");
    assert_eq!(tokens[dollar + 1].kind, TokenKind::Ident);
    tokens[dollar].span.to(tokens[dollar + 1].span)
}

// ------------------------------------------------------------ object-like

#[test]
fn object_like_macro_expands_to_its_body() {
    let source = "define PI 3.14159\nx = $PI;\n";
    assert_eq!(spellings(source), ["x", "=", "3.14159", ";"]);
}

#[test]
fn a_definition_itself_produces_no_tokens() {
    // Only the variable declaration after the definition survives.
    let source = "define PI 3.14159\nint area = 0;\n";
    assert_eq!(spellings(source), ["int", "area", "=", "0", ";"]);
}

#[test]
fn an_object_like_macro_may_have_an_empty_body() {
    let source = "define NOTHING\nx = $NOTHING y;\n";
    assert_eq!(spellings(source), ["x", "=", "y", ";"]);
}

#[test]
fn an_expanded_token_carries_the_invocation_span() {
    // A later diagnostic must point at the `$PI` the programmer wrote, not at
    // the definition's `3.14159`.
    let source = "define PI 3.14159\nx = $PI;\n";
    let expected = invocation_span(source);
    let output = expand(source);
    let expanded = output
        .iter()
        .find(|token| token.spelling() == Some("3.14159"))
        .expect("the body token is in the output");
    // Inside the invocation, so a diagnostic points at `$PI` -- and a span of
    // its own, so that two occurrences in one body can be told apart.
    assert!(
        expanded.span.lo >= expected.lo && expanded.span.lo < expected.hi,
        "{} is not inside {expected:?}",
        expanded.span.lo
    );
    // The tokens around it keep their own positions.
    let x = output
        .iter()
        .find(|token| token.spelling() == Some("x"))
        .expect("`x` is in the output");
    assert_ne!(x.span, expanded.span);
}

#[test]
fn an_expanded_token_has_sensible_remaining_fields() {
    let source = "define PI 3.14159\n$PI\n";
    let output = expand(source);
    let expanded = output
        .iter()
        .find(|token| token.spelling() == Some("3.14159"))
        .expect("the body token is in the output");
    assert!(expanded.leading.is_empty(), "expansion adds no trivia");
    assert!(
        !expanded.newline_before,
        "expansion produces tokens on the invocation's logical line"
    );
}

#[test]
fn the_terminating_eof_is_preserved() {
    let output = expand("define PI 3\n$PI\n");
    assert!(output.last().is_some_and(Token::is_eof));
}

// ----------------------------------------------------------- function-like

#[test]
fn function_like_macro_substitutes_its_arguments() {
    let source = "define MAX(a, b) ((a) > (b) ? (a) : (b))\n$MAX(x, y)\n";
    assert_eq!(
        spellings(source),
        [
            "(", "(", "x", ")", ">", "(", "y", ")", "?", "(", "x", ")", ":", "(", "y", ")", ")"
        ]
    );
}

#[test]
fn a_parameter_may_be_used_more_than_once() {
    let source = "define SQUARE(x) (x) * (x)\n$SQUARE(n)\n";
    assert_eq!(spellings(source), ["(", "n", ")", "*", "(", "n", ")"]);
}

#[test]
fn a_function_like_macro_may_take_no_parameters() {
    let source = "define ANSWER() 42\n$ANSWER()\n";
    assert_eq!(spellings(source), ["42"]);
}

#[test]
fn arguments_are_macro_expanded_before_substitution() {
    // Argument prescan, C semantics: `$ONE` is replaced inside the argument,
    // not left for the substituted body to deal with later.
    let source = "define ONE 1\ndefine DOUBLE(x) (x) + (x)\n$DOUBLE($ONE)\n";
    assert_eq!(spellings(source), ["(", "1", ")", "+", "(", "1", ")"]);
}

#[test]
fn arguments_are_prescanned_recursively() {
    let source = "define ONE 1\ndefine INNER(x) x\ndefine OUTER(x) x\n$OUTER($INNER($ONE))\n";
    assert_eq!(spellings(source), ["1"]);
}

// -------------------------------------------------------------- recursion

#[test]
fn expansion_is_recursive_through_macro_bodies() {
    let source = "define INNER 1\ndefine OUTER $INNER + $INNER\n$OUTER\n";
    assert_eq!(spellings(source), ["1", "+", "1"]);
}

#[test]
fn a_macro_is_not_expanded_inside_its_own_expansion() {
    // The chosen behaviour: a self-recursive macro is reported (L0011) and its
    // inner invocation is left as written. A diagnostic vector is returned, so
    // the caller sees the error rather than a half-expanded stream.
    let source = "define LOOP $LOOP\n$LOOP\n";
    let diagnostics = diagnostics(source);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(codes(source), ["L0011"]);
    assert!(
        diagnostics[0].message.contains("LOOP"),
        "{}",
        diagnostics[0].message
    );
    // The report is inside the invocation: `$LOOP`'s tokens now each have a span
    // of their own, so that two uses of one name in a body are distinguishable.
    let invocation = invocation_span(source);
    assert!(
        diagnostics[0].span.lo >= invocation.lo && diagnostics[0].span.lo < invocation.hi,
        "{:?} is not inside {invocation:?}",
        diagnostics[0].span
    );
}

#[test]
fn a_cycle_of_macros_is_reported_as_self_recursion() {
    let source = "define A $B\ndefine B $A\n$B\n";
    assert_eq!(codes(source), ["L0011"]);
}

#[test]
fn expansion_stops_at_the_depth_limit() {
    // 128 nested invocations are one too many; 127 are allowed.
    let deep = nested_chain(128);
    assert_eq!(codes(&deep), ["L0010"]);
    let allowed = nested_chain(127);
    assert_eq!(spellings(&allowed), ["end"]);
}

/// Builds a chain of `links` macros, each invoking the next, ending in `end`.
fn nested_chain(links: usize) -> String {
    let mut source = String::new();
    for index in 0..links {
        source.push_str(&format!("define M{index} $M{}\n", index + 1));
    }
    source.push_str(&format!("define M{links} end\n"));
    source.push_str("$M0\n");
    source
}

// ----------------------------------------------------------- stringify (`#`)

#[test]
fn hash_stringifies_a_parameter() {
    let source = "define STR(x) #x\n$STR(a + b)\n";
    assert_eq!(spellings(source), ["\"a + b\""]);
}

#[test]
fn stringification_has_a_single_space_between_tokens() {
    // Whitespace in the argument is invisible in the token stream, so the
    // string is rebuilt with one space between tokens and no padding.
    let source = "define STR(x) #x\n$STR(  a   +    b  )\n";
    assert_eq!(spellings(source), ["\"a + b\""]);
}

#[test]
fn stringification_escapes_quotes_and_backslashes() {
    // The argument is one raw-string token; its text contains `"` and `\`, and
    // both must be escaped to keep the result a single valid string literal.
    let source = "define STR(x) #x\n$STR(r\"C:\\dir\")\n";
    assert_eq!(spellings(source), [r#""r\"C:\\dir\"""#]);
}

#[test]
fn stringification_does_not_prescan_its_argument() {
    // `#` takes the argument as written: `$ONE` is text, not `1`. The sigil is
    // a token of its own here, so the single-space rule applies between `$` and
    // `ONE`, exactly as it would between any two tokens.
    let source = "define ONE 1\ndefine STR(x) #x\n$STR($ONE)\n";
    assert_eq!(spellings(source), ["\"$ ONE\""]);
}

#[test]
fn an_empty_argument_stringifies_to_an_empty_string() {
    let source = "define STR(x) #x\n$STR()\n";
    assert_eq!(spellings(source), ["\"\""]);
}

// -------------------------------------------------------------- paste (`##`)

#[test]
fn paste_joins_two_identifiers() {
    let source = "define CAT(a, b) a ## b\n$CAT(foo, bar)\n";
    assert_eq!(spellings(source), ["foobar"]);
}

#[test]
fn paste_joins_an_identifier_and_a_number() {
    let source = "define NAME(i) x ## i\n$NAME(1)\n";
    assert_eq!(spellings(source), ["x1"]);
}

#[test]
fn paste_joins_two_numbers_into_a_number() {
    let source = "define TWO(a, b) a ## b\n$TWO(1, 2)\n";
    let output = expand(source);
    let pasted = &output[0];
    assert_eq!(pasted.kind, TokenKind::Int);
    assert_eq!(spelling(pasted), "12");
}

#[test]
fn paste_ignores_whitespace_around_the_operator() {
    let source = "define CAT(a, b) a  ##  b\n$CAT(foo, bar)\n";
    assert_eq!(spellings(source), ["foobar"]);
}

#[test]
fn paste_can_form_punctuation() {
    let source = "define PLUSPLUS() + ## +\n$PLUSPLUS()\n";
    assert_eq!(spellings(source), ["++"]);
}

#[test]
fn paste_with_an_empty_argument_leaves_the_other_token() {
    // C's placemarker rule: an empty argument next to `##` contributes nothing.
    let source = "define CAT(a, b) a ## b\n$CAT(, bar)\n";
    assert_eq!(spellings(source), ["bar"]);
    let source = "define CAT(a, b) a ## b\n$CAT(foo, )\n";
    assert_eq!(spellings(source), ["foo"]);
}

#[test]
fn paste_with_both_arguments_empty_produces_nothing() {
    let source = "define CAT(a, b) a ## b\n$CAT(,)\n";
    assert!(spellings(source).is_empty(), "{:?}", spellings(source));
}

#[test]
fn a_paste_that_is_not_one_token_is_an_error() {
    let source = "define CAT(a, b) a ## b\n$CAT(+, -)\n";
    assert_eq!(codes(source), ["L0012"]);
}

#[test]
fn a_paste_at_the_start_of_a_body_is_an_error() {
    assert_eq!(codes("define BAD(a) ## a\n"), ["L0007"]);
}

#[test]
fn a_paste_at_the_end_of_a_body_is_an_error() {
    assert_eq!(codes("define BAD(a) a ##\n"), ["L0007"]);
}

// ---------------------------------------------------------------- variadics

#[test]
fn a_variadic_macro_expands_its_list_with_commas() {
    let source = "define CALL(name, ...) name(__VA_LIST__)\n$CALL(print, 1, 2)\n";
    assert_eq!(spellings(source), ["print", "(", "1", ",", "2", ")"]);
}

#[test]
fn a_variadic_macro_with_no_variadic_arguments_expands_the_list_to_nothing() {
    let source = "define CALL(name, ...) name(__VA_LIST__)\n$CALL(print)\n";
    assert_eq!(spellings(source), ["print", "(", ")"]);
}

#[test]
fn a_variadic_macro_may_have_no_fixed_parameters() {
    let source = "define LIST(...) [__VA_LIST__]\n$LIST(a, b)\n";
    assert_eq!(spellings(source), ["[", "a", ",", "b", "]"]);
}

#[test]
fn a_variadic_macro_with_no_fixed_parameters_accepts_an_empty_list() {
    // `()` is zero arguments here because the macro declares no parameters.
    let source = "define LIST(...) [__VA_LIST__]\n$LIST()\n";
    assert_eq!(spellings(source), ["[", "]"]);
}

#[test]
fn too_few_arguments_for_a_variadic_macro_is_an_error() {
    // `()` is one empty argument for the fixed parameter, which is allowed; a
    // call that leaves a fixed parameter out entirely is not.
    let source = "define CALL(name, ...) name(__VA_LIST__)\n$CALL()\n";
    assert_eq!(spellings(source), ["(", ")"]);
    let source = "define F(a, b, ...) a b\n$F(1)\n";
    assert_eq!(codes(source), ["L0006"]);
}

#[test]
fn stringifying_the_variadic_list_is_an_error() {
    // `#` stringifies a named parameter; `__VA_LIST__` is not one, and the task
    // does not ask for `#__VA_LIST__`.
    let source = "define STRV(...) #__VA_LIST__\n";
    assert_eq!(codes(source), ["L0007"]);
}

#[test]
fn the_variadic_list_is_a_parameter_only_in_a_variadic_macro() {
    // In a macro without `...`, `__VA_LIST__` is an ordinary name.
    let source = "define PLAIN() __VA_LIST__\n$PLAIN()\n";
    assert_eq!(spellings(source), ["__VA_LIST__"]);
}

// ------------------------------------------------------- line continuation

#[test]
fn a_backslash_newline_continues_a_definition() {
    let source = "define LONG a + \\\n    b\n$LONG;\n";
    // The continuation is not a token and produces no diagnostic.
    let mut lexer = Lexer::default();
    let lexed = lexer.tokenize(source, FileId::from_raw(0));
    assert!(lexed.diagnostics.is_empty(), "{:?}", lexed.diagnostics);
    assert_eq!(spellings(source), ["a", "+", "b", ";"]);
}

#[test]
fn a_definition_stops_at_the_end_of_its_line() {
    // The next line's tokens are code, not part of the body.
    let source = "define PI 3.14159\nint x = $PI;\n";
    assert_eq!(spellings(source), ["int", "x", "=", "3.14159", ";"]);
}

// ------------------------------------------------------------------- export

#[test]
fn export_define_is_a_definition() {
    let source = "export define E int\n$E x;\n";
    assert_eq!(spellings(source), ["int", "x", ";"]);
}

#[test]
fn an_export_that_is_not_a_definition_is_passed_through() {
    let source = "export int x;\n";
    assert_eq!(spellings(source), ["export", "int", "x", ";"]);
}

// -------------------------------------------------------- invocation sites

#[test]
fn a_macro_can_expand_inside_a_type() {
    // `$ELEM *p;` is the task's example: the expansion is tokens, so nothing
    // about it knows what a type is.
    let source = "define ELEM u32\n$ELEM *p;\n";
    let output = expand(source);
    assert_eq!(tokens_to_spellings(&output), ["u32", "*", "p", ";"]);
    let invocation = invocation_span(source);
    assert!(
        output[0].span.lo >= invocation.lo && output[0].span.lo < invocation.hi,
        "{} is not inside {invocation:?}",
        output[0].span.lo
    );
}

#[test]
fn a_macro_can_expand_at_item_position() {
    let source = "define MAIN fn main() int { return 0; }\n$MAIN\n";
    assert_eq!(
        spellings(source),
        ["fn", "main", "(", ")", "int", "{", "return", "0", ";", "}"]
    );
}

// ---------------------------------------------------------- the token stream

#[test]
fn only_parentheses_group_commas_in_an_argument_list() {
    // The task specifies token-balanced *parentheses*; braces are ordinary
    // tokens, so a comma inside them still separates arguments, as in C. Three
    // parameters is what makes the pin exact: if `{1, 2}` grouped, there would
    // be two arguments and this would be an arity error instead.
    let source = "define THREE(a, b, c) c\n$THREE({1, 2}, 3)\n";
    assert_eq!(spellings(source), ["3"]);
}

#[test]
fn a_comma_inside_nested_parentheses_does_not_separate_arguments() {
    let source = "define FIRST(a, b) a\n$FIRST((1, 2), 3)\n";
    assert_eq!(spellings(source), ["(", "1", ",", "2", ")"]);
}

#[test]
fn empty_arguments_are_allowed() {
    let source = "define PAIR(a, b) a b\n$PAIR(, x)\n";
    assert_eq!(spellings(source), ["x"]);
}

#[test]
fn a_one_parameter_macro_accepts_an_empty_argument() {
    let source = "define SQUARE(x) (x) * (x)\n$SQUARE()\n";
    assert_eq!(spellings(source), ["(", ")", "*", "(", ")"]);
}

#[test]
fn whitespace_between_the_name_and_the_argument_list_is_ignored() {
    let source = "define PAIR(a, b) a b\n$PAIR ( x , y )\n";
    assert_eq!(spellings(source), ["x", "y"]);
}

#[test]
fn a_space_before_the_parameter_list_makes_a_macro_object_like() {
    // C's rule: the `(` of a function-like definition must immediately follow
    // the name, so this defines an object-like macro whose body is `(x) x`.
    let source = "define F (x) x\n$F\n";
    assert_eq!(spellings(source), ["(", "x", ")", "x"]);
}

#[test]
fn an_argument_list_may_follow_on_the_next_line() {
    // The invocation is decided by tokens, not layout: `$PAIR` followed by a
    // parenthesised list is an invocation even when a newline intervenes.
    let source = "define PAIR(a, b) a b\n$PAIR\n( x, y )\n";
    assert_eq!(spellings(source), ["x", "y"]);
}

#[test]
fn an_attribute_is_passed_through_and_its_argument_expands() {
    // `@` needs no macro treatment; an invocation inside an attribute is still
    // an invocation.
    let source = "define ELEM int\n@alignas($ELEM) int x;\n";
    assert_eq!(
        spellings(source),
        ["@", "alignas", "(", "int", ")", "int", "x", ";"]
    );
}

#[test]
fn expansion_output_is_not_rescanned_for_definitions() {
    // A body may contain the word `define`; it is copied out as a name, not
    // treated as a definition. Only the file's own stream is scanned for them.
    let source = "define WORD define\n$WORD\n";
    assert_eq!(spellings(source), ["define"]);
}

#[test]
fn a_definition_is_visible_only_after_it_is_written() {
    // The stream is processed in order, as in C; a use before the definition is
    // an unknown macro.
    let source = "$A\ndefine A 1\n";
    assert_eq!(codes(source), ["L0003"]);
}

// ------------------------------------------------------------------- errors

#[test]
fn an_unknown_macro_is_an_error() {
    let source = "x = $NOPE;\n";
    assert_eq!(codes(source), ["L0003"]);
    assert_eq!(diagnostics(source)[0].span, invocation_span(source));
}

#[test]
fn a_dollar_not_followed_by_a_name_is_an_error() {
    assert_eq!(codes("$;\n"), ["L0002"]);
    assert_eq!(codes("$"), ["L0002"]);
}

#[test]
fn a_dollar_followed_by_a_keyword_is_an_error() {
    // `$if` does not name a macro: keywords are not names.
    assert_eq!(codes("$if\n"), ["L0002"]);
}

#[test]
fn invoking_a_function_like_macro_without_arguments_is_an_error() {
    assert_eq!(codes("define F(x) x\n$F\n"), ["L0005"]);
}

#[test]
fn invoking_an_object_like_macro_with_arguments_is_an_error() {
    assert_eq!(codes("define PI 3\n$PI(1)\n"), ["L0004"]);
}

#[test]
fn a_wrong_argument_count_is_an_error() {
    assert_eq!(codes("define F(a, b) a b\n$F(1)\n"), ["L0006"]);
    assert_eq!(codes("define F(a, b) a b\n$F(1, 2, 3)\n"), ["L0006"]);
    assert_eq!(codes("define F() 1\n$F(1)\n"), ["L0006"]);
}

#[test]
fn an_unterminated_argument_list_is_an_error() {
    assert_eq!(codes("define F(a) a\n$F(1\n"), ["L0013"]);
}

#[test]
fn a_redefinition_is_an_error() {
    assert_eq!(codes("define A 1\ndefine A 2\n$A\n"), ["L0008"]);
}

#[test]
fn a_definition_name_that_is_not_an_identifier_is_an_error() {
    assert_eq!(codes("define 42\n"), ["L0007"]);
    assert_eq!(codes("define\n"), ["L0007"]);
}

#[test]
fn a_malformed_parameter_list_is_an_error() {
    assert_eq!(codes("define F(a, ..., b) a\n"), ["L0007"]);
    assert_eq!(codes("define F(a, a) a\n"), ["L0007"]);
    assert_eq!(codes("define F(42) a\n"), ["L0007"]);
    assert_eq!(codes("define F(a b) a\n"), ["L0007"]);
}

#[test]
fn hash_outside_a_macro_body_is_an_error() {
    assert_eq!(codes("x # y\n"), ["L0009"]);
}

#[test]
fn hash_must_be_followed_by_a_parameter_inside_a_body() {
    assert_eq!(codes("define BAD(x) #y\n"), ["L0007"]);
    // An object-like macro has nothing to stringify.
    assert_eq!(codes("define BAD #x\n"), ["L0007"]);
}

#[test]
fn a_define_that_does_not_begin_a_line_is_an_ordinary_name() {
    // `define` is not a reserved word. Its only special position is the start of
    // a definition, which is the start of a logical line; as a variable name in
    // an item it is just a name.
    let source = "int define = 1;\n";
    assert_eq!(spellings(source), ["int", "define", "=", "1", ";"]);
}

#[test]
fn a_definition_after_another_item_on_the_same_line_is_not_recognised() {
    // The approximation is the documented limit: a token preprocessor cannot
    // see item position, so a definition must begin its line. Here `define` is
    // left as a name and `$A` is therefore an unknown macro.
    let source = "fn f() {} define A 1\n$A\n";
    assert_eq!(codes(source), ["L0003"]);
}

#[test]
fn a_malformed_definition_does_not_leak_into_the_output() {
    // Recovery drops the definition's line so the parser is not handed
    // `define 42` as if it were an expression.
    let source = "define 42\nx = 1;\n";
    assert_eq!(codes(source), ["L0007"]);
}

// -------------------------------------------------------------- diagnostics

#[test]
fn every_error_is_collected_not_just_the_first() {
    // Two independent unknown macros are both reported, so a caller can show
    // the programmer everything wrong in one pass.
    let source = "$ONE $TWO\n";
    assert_eq!(codes(source), ["L0003", "L0003"]);
}

// ------------------------------------------------------------- raw strings

#[test]
fn a_hash_inside_a_raw_string_is_never_a_macro_operator() {
    // `r#q"a # b"q#` is one token; its `#` characters are just text.
    let source = "define R r#q\"a # b\"q#\n$R\n";
    let output = expand(source);
    assert_eq!(spellings(source), [r#"r#q"a # b"q#"#]);
    assert_eq!(output[0].kind, TokenKind::RawString);
}
