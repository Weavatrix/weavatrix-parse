//! Source tokenizer and structural extractor for repository intelligence.
//!
//! This crate exists because line-oriented scanning is wrong in ways that
//! matter: a route written inside a comment becomes an endpoint, a `//` inside
//! a string ends the line early, a declaration spanning three lines
//! disappears, and a class body yields no methods. Every one of those is a
//! tokenizer problem, so this crate starts with a tokenizer.
//!
//! It deliberately does not build an expression tree. Repository intelligence
//! consumes declarations, imports, exports, calls and their spans - not
//! operator precedence - so the structural pass walks the token stream and
//! stops there. That keeps the crate small enough to own outright and fast
//! enough to run over every file of a monorepo.
//!
//! No dependencies, no generated grammars, no C, `unsafe` forbidden.

pub mod syntax;
pub mod token;

pub use syntax::{Language, Syntax};
pub use token::{Mode, Token, TokenKind, Tokenizer, tokenize, tokenize_lite};
