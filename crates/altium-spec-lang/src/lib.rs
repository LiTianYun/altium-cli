//! The document-independent Altium spec language front end.
//!
//! Owns lossless syntax trees, AST parsing, source diagnostics, expression
//! evaluation, trivia preservation, and canonical formatting. The legacy
//! `altium-format-spec` crate reexports these modules for existing consumers.
//!
//! Authored intent and semantic compilation are still being migrated according
//! to `NEXT.md`; document projection and planning belong to `altium-sync`.

pub mod ast;
pub mod cst;
pub mod diagnostic;
pub mod eval;
pub mod formatter;
pub mod import;
pub mod lexer;
pub mod parser;
pub mod trivia;

pub use cst::{DumpMergeError, merge_dump};
pub use eval::{
    EvalResult, ScopeStack, Severity, Shape, SpecError, SpecErrorCode, Value, eval_expr,
    eval_let_bindings, unit_to_internal,
};
pub use formatter::{FormatConfig, FormatResult, extract_top_level_trivia, format_spec};
pub use import::{ResolvedSpec, resolve_imports};
pub use trivia::{
    CommentToken, ItemTrivia, TriviaLine, TriviaMap, parse_with_trivia, scan_trivia_lines,
};
