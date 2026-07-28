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

pub mod braced;
pub mod docs;
pub mod facts;
pub mod hcl;
pub mod markup;
pub mod python;
pub mod script;
pub mod sql;
pub mod style;
pub mod syntax;
pub mod token;

pub use facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
pub use syntax::{Language, Syntax};
pub use token::{Mode, Token, TokenKind, Tokenizer, tokenize, tokenize_lite};

/// Extracts structural facts from one source file.
///
/// Languages this crate can tokenize but has no structural model for yet -
/// shell and YAML - return no facts rather than guesses.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    match language {
        Language::JavaScript | Language::TypeScript => script::extract(source, language),
        Language::Python => python::extract(source),
        Language::Sql => sql::extract(source),
        Language::Terraform => hcl::extract(source),
        Language::Markdown | Language::Mdx | Language::ReStructuredText | Language::AsciiDoc => {
            docs::extract(source, language)
        }
        Language::Html | Language::Xml => markup::extract(source, language),
        Language::Css | Language::Scss => style::extract(source, language),
        Language::Rust
        | Language::Go
        | Language::Java
        | Language::CSharp
        | Language::C
        | Language::Cpp
        | Language::Solidity
        | Language::Swift => braced::extract(source, language),
        _ => Facts::default(),
    }
}

/// Extracts structural facts from a file, choosing the language by extension.
#[must_use]
pub fn extract_path(path: &str, source: &str) -> Option<Facts> {
    let extension = path.rsplit_once('.')?.1;
    let language = Language::from_extension(extension)?;
    Some(extract(source, language))
}
