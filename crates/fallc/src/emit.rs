//! Rendering stage output for the user.
//!
//! Kept separate from the stages themselves so that library users never
//! inherit a text format they did not ask for, and so that changing the format
//! cannot change compiler behaviour.

use fallc_ast::Ast;
use fallc_lexer::LexResult;
use fallc_sema::TypeTable;
use fallc_span::SourceMap;

/// Renders a token stream, one token per line.
///
/// Each line is `kind  span  text`, with leading trivia indented beneath.
#[must_use]
pub fn tokens(result: &LexResult, sources: &SourceMap) -> String {
    let mut out = String::new();
    for token in &result.tokens {
        for trivia in &token.leading {
            out.push_str(&format!("  trivia {}\n", sources.describe(trivia.span())));
        }
        let text = sources
            .file(token.span.file)
            .and_then(|f| f.slice(token.span))
            .unwrap_or("");
        out.push_str(&format!(
            "{:<16} {:<24} {:?}\n",
            token.kind.describe(),
            sources.describe(token.span),
            text
        ));
    }
    out
}

/// Renders an AST.
#[must_use]
pub fn ast(ast: &Ast) -> String {
    fallc_ast::pretty::dump(ast)
}

/// Renders LLVM IR.
///
/// The text is already produced by LLVM itself (`Module::print_to_string`), so
/// this only applies the header the driver wants; it must never reformat the
/// body, or the output would stop being valid LLVM assembly.
#[must_use]
pub fn llvm_ir(ir: &str) -> String {
    if ir.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(ir.len() + 1);
    out.push_str(ir);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Renders a type table.
#[must_use]
pub fn types(table: &TypeTable, sources: &SourceMap) -> String {
    let mut out = String::new();
    if table.is_empty() {
        out.push_str("(no types inferred)\n");
        return out;
    }
    for (span, kind) in table.entries_for_display() {
        out.push_str(&format!("{:<24} {}\n", sources.describe(span), kind));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallc_lexer::{Lexer, LexerConfig};
    use fallc_span::FileId;

    #[test]
    fn token_rendering_includes_eof() {
        let mut sources = SourceMap::new();
        let id = sources.add_file("t.ft", "");
        let lexed = Lexer::new(LexerConfig::default()).tokenize("", id);
        let text = tokens(&lexed, &sources);
        assert!(text.contains("end of input"), "{text}");
    }

    #[test]
    fn empty_type_table_says_so() {
        let sources = SourceMap::new();
        assert_eq!(types(&TypeTable::new(), &sources), "(no types inferred)\n");
    }

    #[test]
    fn empty_ast_renders_empty() {
        assert_eq!(ast(&Ast::new()), "");
    }

    #[test]
    fn empty_ir_renders_nothing() {
        assert_eq!(llvm_ir(""), "");
    }

    #[test]
    fn ir_gets_a_trailing_newline() {
        assert_eq!(llvm_ir("define void @f() {}\n"), "define void @f() {}\n");
        assert_eq!(llvm_ir("define void @f() {}"), "define void @f() {}\n");
    }

    #[test]
    fn file_id_is_unused_but_kept_for_clarity() {
        // Guards against the import being dropped accidentally when the
        // rendering format changes.
        let _ = FileId::from_raw(0);
    }
}
