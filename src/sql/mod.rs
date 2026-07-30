//! Structural extraction for SQL.
//!
//! SQL declares objects rather than functions and depends on other objects by
//! name rather than by path, so the fact shapes mean something slightly
//! different here: a `CREATE` is a declaration, and every table a statement
//! reads or writes is an import whose specifier is the object name. That is
//! what makes a view resolvable to the file that creates the table it selects
//! from, which is the edge repository intelligence actually wants.
//!
//! Keywords are matched case-insensitively because SQL is written both ways,
//! often in the same file.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one SQL source.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Sql)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        object: None,
    };
    state.run();
    state.facts
}

/// The object keyword to the kind it declares.
const OBJECTS: &[(&str, DeclarationKind)] = &[
    ("table", DeclarationKind::Table),
    ("view", DeclarationKind::View),
    ("function", DeclarationKind::Function),
    ("procedure", DeclarationKind::Procedure),
    ("trigger", DeclarationKind::Procedure),
    ("schema", DeclarationKind::Module),
    ("type", DeclarationKind::TypeAlias),
];

/// Words that qualify a `CREATE` without naming what it creates.
const CREATE_MODIFIERS: &[&str] = &[
    "or",
    "replace",
    "temp",
    "temporary",
    "unique",
    "materialized",
    "global",
    "local",
    "if",
    "not",
    "exists",
];

/// Keywords a referenced object name follows.
const REFERENCES: &[&str] = &["from", "join", "into", "update", "references", "on"];

/// Words that read as an object name but never are one.
const NOT_A_NAME: &[&str] = &[
    "select",
    "lateral",
    "only",
    "delete",
    "conflict",
    "duplicate",
    "set",
    "values",
    "all",
    "distinct",
];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// The object being created, which owns everything until the statement ends.
    object: Option<String>,
}

mod extractor;

#[cfg(test)]
mod tests;
