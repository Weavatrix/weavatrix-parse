//! Structural extraction for Python.
//!
//! Python scopes by indentation rather than braces, so the walk tracks the
//! column a declaration was written at and closes it when a later declaration
//! appears at the same column or further left. Working from token columns
//! rather than raw line prefixes keeps this correct inside triple-quoted
//! strings, where a line that looks like `def x():` is text, not code.

use crate::facts::{
    Declaration, DeclarationKind, Facts, Import, ImportBinding, Reference, ReferenceKind, Span,
};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one Python source.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Python)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        scopes: Vec::new(),
    };
    state.run();
    state.facts
}

/// A `def` or `class` whose indented body the walk is inside.
struct Scope {
    name: String,
    column: u32,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    scopes: Vec<Scope>,
}

mod extractor;

#[cfg(test)]
mod tests;
