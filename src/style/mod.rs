//! Structural extraction for CSS, SCSS, Sass and Less.
//!
//! A stylesheet is a reference graph rather than a call graph: what matters is
//! which selectors a file declares, because that is what an HTML `class` or
//! `id` attribute resolves to, and which other stylesheets it pulls in.
//!
//! Selectors are read from the token stream rather than by matching lines,
//! which is what makes nesting work. In SCSS a rule written inside another
//! rule is a real selector, and a `&` prefix concatenates it onto its parent -
//! so `.card { &__title { } }` declares `.card__title`, a name that appears
//! nowhere in the source as written.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one stylesheet.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        nesting: Vec::new(),
    };
    state.run();
    state.facts
}

/// At-rules that name another stylesheet.
const AT_IMPORTS: &[&str] = &["import", "use", "forward"];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// Selector prefixes of the enclosing rules, one per open brace.
    nesting: Vec<String>,
}

/// Records that a document uses a selector, which is what an HTML `class` or
/// `id` attribute does. Kept here so the HTML extractor and this one agree on
/// how a selector is named.
pub(crate) fn selector_use(facts: &mut Facts, name: String, span: Span) {
    facts.references.push(Reference {
        name,
        kind: ReferenceKind::Uses,
        receiver: None,
        span,
        owner: None,
        string_arguments: Vec::new(),
        name_arguments: Vec::new(),
    });
}

mod extractor;

#[cfg(test)]
mod tests;
