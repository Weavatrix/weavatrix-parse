//! Structural extraction for HTML and single-file components.
//!
//! HTML contributes two kinds of edge. A `<link href>`, `<script src>` or
//! `<img src>` names another file, which is an ordinary dependency. A `class`
//! or `id` attribute names a CSS selector, which resolves to whichever
//! stylesheet declares it - an edge that exists only once both sides are
//! parsed, and the reason a document and its stylesheet belong in one graph.
//!
//! Attributes are read from the token stream, so a tag written inside a
//! comment contributes nothing, and a `<` inside an attribute value does not
//! open a tag.

use crate::facts::{Facts, Import, Span};
use crate::style::selector_use;
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one document.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        language,
        facts: Facts::default(),
        tag: String::new(),
        text_start: None,
    };
    state.run();
    state.facts
}

/// Elements whose text is a dependency rather than prose.
///
/// XML project files name their dependencies in element content rather than in
/// attributes: a Maven module lists `<module>ui</module>`, and an artifact is
/// split across `<groupId>` and `<artifactId>`.
fn names_a_file_in_text(tag: &str) -> bool {
    matches!(tag, "module" | "include" | "xi:include" | "systemid")
}

/// Which attribute names a file, given the tag it is written on.
///
/// `href` on an anchor is a link to a page rather than a dependency of this
/// one, so the tag decides, not the attribute alone.
fn names_a_file(language: Language, tag: &str, attribute: &str) -> bool {
    if language == Language::Xml {
        // A project file points at another project or package by attribute:
        // `<ProjectReference Include="../Lib/Lib.csproj">`, `<xi:include
        // href="shared.xml">`, `<xsd:import schemaLocation="types.xsd">`.
        return matches!(
            attribute,
            "include" | "href" | "src" | "schemalocation" | "location" | "file" | "path"
        );
    }
    match attribute {
        "href" => matches!(tag, "link" | "use"),
        "src" => matches!(
            tag,
            "script" | "img" | "iframe" | "audio" | "video" | "source" | "embed" | "track"
        ),
        "srcset" | "data-src" => true,
        _ => false,
    }
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
    facts: Facts,
    /// The tag whose attributes are being read.
    tag: String,
    /// Where the text of an element whose content names a file begins.
    text_start: Option<(usize, String)>,
}

mod extractor;

#[cfg(test)]
mod tests;
