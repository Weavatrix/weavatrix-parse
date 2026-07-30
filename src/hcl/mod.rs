//! Structural extraction for Terraform and HCL.
//!
//! Infrastructure is a dependency graph that no other extractor here can see.
//! A `module` block names another directory of configuration; a `source` in
//! `required_providers` names a registry package; and every `var.x`,
//! `module.m.out` and `aws_s3_bucket.b.id` is a reference from one declared
//! object to another. Those are the same edges a code graph carries, drawn
//! over a part of the repository that has been invisible until now.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one Terraform or HCL file.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Terraform)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        block: Vec::new(),
        depth: 0,
    };
    state.run();
    state.facts
}

/// Block types whose labels name the object they declare.
const DECLARING: &[(&str, DeclarationKind)] = &[
    ("resource", DeclarationKind::Resource),
    ("data", DeclarationKind::Resource),
    ("module", DeclarationKind::Module),
    ("variable", DeclarationKind::Variable),
    ("output", DeclarationKind::Resource),
    ("provider", DeclarationKind::Module),
    ("locals", DeclarationKind::Constant),
];

/// Prefixes that introduce a reference to another declared object.
const REFERENCE_ROOTS: &[&str] = &["var", "module", "data", "local", "each"];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// Names of the enclosing blocks, innermost last.
    block: Vec<String>,
    depth: i32,
}

mod extractor;

#[cfg(test)]
mod tests;
