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
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Html)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        tag: String::new(),
    };
    state.run();
    state.facts
}

/// Which attribute names a file, given the tag it is written on.
///
/// `href` on an anchor is a link to a page rather than a dependency of this
/// one, so the tag decides, not the attribute alone.
fn names_a_file(tag: &str, attribute: &str) -> bool {
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
    facts: Facts,
    /// The tag whose attributes are being read.
    tag: String,
}

impl Extractor<'_, '_> {
    fn run(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            index = self.step(index);
        }
    }

    fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let last_index = self.tokens.len().saturating_sub(1);
        let first = &self.tokens[start.min(last_index)];
        let last = &self.tokens[end.min(last_index)];
        Span {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.line,
            end_column: last.column,
        }
    }

    fn step(&mut self, index: usize) -> usize {
        // `<tag` opens an element and names the tag the following attributes
        // belong to; `</` and `>` end one.
        if self.punct(index, "<") {
            if self.kind(index + 1) == Some(TokenKind::Identifier) {
                self.tag = self.text(index + 1).to_ascii_lowercase();
                return index + 2;
            }
            self.tag.clear();
            return index + 1;
        }
        if self.punct(index, ">") {
            self.tag.clear();
            return index + 1;
        }
        if self.tag.is_empty() {
            return index + 1;
        }
        self.attribute(index)
    }

    /// `name="value"`, `name='value'` or `name=value`.
    fn attribute(&mut self, index: usize) -> usize {
        if self.kind(index) != Some(TokenKind::Identifier) || !self.punct(index + 1, "=") {
            return index + 1;
        }
        // A namespace prefix does not change what the attribute means:
        // `xlink:href` names a file exactly as `href` does.
        let written = self.text(index).to_ascii_lowercase();
        let name = written
            .rsplit_once(':')
            .map_or(written.as_str(), |(_, local)| local)
            .to_owned();
        let value_index = index + 2;
        let raw = self.text(value_index);
        // Owned before any push, because the borrow of the token text and the
        // borrow of the fact list are both of `self`.
        let value = match self.kind(value_index) {
            Some(TokenKind::String) => raw.trim_matches(['"', '\'']).to_owned(),
            Some(TokenKind::Identifier | TokenKind::Number) => raw.to_owned(),
            _ => return index + 2,
        };
        if value.is_empty() {
            return value_index + 1;
        }
        let span = self.span(index, value_index);
        match name.as_str() {
            "class" => {
                for class in value.split_whitespace() {
                    selector_use(&mut self.facts, format!(".{class}"), span);
                }
            }
            "id" => selector_use(&mut self.facts, format!("#{value}"), span),
            _ if names_a_file(&self.tag, &name) => {
                // A srcset lists several candidates with descriptors.
                for candidate in value.split(',') {
                    let path = candidate.split_whitespace().next().unwrap_or("");
                    // A data URI or an external URL is not a file in this tree.
                    if path.is_empty() || path.contains(':') || path.starts_with("//") {
                        continue;
                    }
                    self.facts.imports.push(Import {
                        specifier: path.to_owned(),
                        span,
                        type_only: false,
                        reexport: false,
                    });
                }
            }
            _ => {}
        }
        value_index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::ReferenceKind;

    fn imports(source: &str) -> Vec<String> {
        extract(source)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect()
    }

    fn uses(source: &str) -> Vec<String> {
        extract(source)
            .references
            .into_iter()
            .filter(|reference| reference.kind == ReferenceKind::Uses)
            .map(|reference| reference.name)
            .collect()
    }

    #[test]
    fn a_document_depends_on_the_files_it_pulls_in() {
        let source = "<html>\n\
             <head>\n\
             <link rel=\"stylesheet\" href=\"./styles/app.css\">\n\
             <script src=\"/js/main.js\"></script>\n\
             </head>\n\
             <body>\n\
             <img src=\"assets/logo.png\" alt=\"logo\">\n\
             <a href=\"/about\">about</a>\n\
             <script src=\"https://cdn.example.com/x.js\"></script>\n\
             </body>\n\
             </html>\n";
        assert_eq!(
            imports(source),
            ["./styles/app.css", "/js/main.js", "assets/logo.png"],
            "an anchor is navigation and a CDN script is not a file in this tree"
        );
    }

    #[test]
    fn class_and_id_attributes_use_the_selectors_a_stylesheet_declares() {
        let source = "<div class=\"panel panel--wide\" id=\"root\">\n\
             <span class=\"badge\">x</span>\n\
             </div>\n";
        assert_eq!(
            uses(source),
            [".panel", ".panel--wide", "#root", ".badge"],
            "a class attribute names one selector per word"
        );
    }

    #[test]
    fn a_tag_written_inside_a_comment_contributes_nothing() {
        let source = "<!-- <script src=\"./ghost.js\"></script> -->\n\
             <script src=\"./real.js\"></script>\n";
        assert_eq!(imports(source), ["./real.js"]);
    }

    #[test]
    fn an_angle_bracket_inside_a_value_does_not_open_a_tag() {
        let source = "<div data-tpl=\"<b>bold</b>\" class=\"kept\"></div>\n";
        assert_eq!(
            uses(source),
            [".kept"],
            "the attribute value is one string token, not markup"
        );
    }

    #[test]
    fn a_srcset_names_every_candidate_without_its_descriptor() {
        assert_eq!(
            imports("<img srcset=\"small.png 480w, large.png 1080w\" src=\"fallback.png\">\n"),
            ["small.png", "large.png", "fallback.png"]
        );
    }

    #[test]
    fn hyphenated_and_namespaced_attributes_are_read_as_one_name() {
        let source = "<use xlink:href=\"./sprite.svg#icon\"></use>\n\
             <div data-count=\"3\" aria-label=\"n\" class=\"c\"></div>\n";
        assert_eq!(imports(source), ["./sprite.svg#icon"]);
        assert_eq!(uses(source), [".c"]);
    }
}
