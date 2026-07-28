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
        if self.punct(index, "}") {
            self.nesting.pop();
            return index + 1;
        }
        if (self.punct(index, "@") || self.text(index).starts_with('@'))
            && let Some(next) = self.at_rule(index)
        {
            return next;
        }
        // A selector list runs up to the brace that opens its block. Anything
        // else at this level is a property declaration, which ends at a
        // semicolon and declares nothing.
        if let Some(open) = self.selector_block(index) {
            return open;
        }
        index + 1
    }

    /// `@import "x.css"`, `@use "sass:math"`, `@forward "./theme"`.
    fn at_rule(&mut self, index: usize) -> Option<usize> {
        // The tokenizer gives `@` separately in CSS and joined in SCSS, where
        // `@` is an identifier character, so both shapes are accepted.
        let (keyword, mut cursor) = if self.punct(index, "@") {
            (self.text(index + 1).to_owned(), index + 2)
        } else {
            (
                self.text(index).trim_start_matches('@').to_owned(),
                index + 1,
            )
        };
        if !AT_IMPORTS
            .iter()
            .any(|word| word.eq_ignore_ascii_case(&keyword))
        {
            return None;
        }
        let limit = (cursor + 64).min(self.tokens.len());
        let mut found = false;
        while cursor < limit && !self.punct(cursor, ";") && !self.punct(cursor, "{") {
            if self.kind(cursor) == Some(TokenKind::String) {
                let specifier = self.text(cursor).trim_matches(['"', '\'']).to_owned();
                if !specifier.is_empty() {
                    self.facts.imports.push(Import {
                        specifier,
                        span: self.span(index, cursor),
                        type_only: false,
                        reexport: keyword.eq_ignore_ascii_case("forward"),
                        names: Vec::new(),
                    });
                    found = true;
                }
            }
            cursor += 1;
        }
        found.then_some(cursor)
    }

    /// Reads a selector list ending at `{`, records every class and id it
    /// names, and pushes the nesting prefix for the block it opens.
    fn selector_block(&mut self, start: usize) -> Option<usize> {
        let limit = (start + 256).min(self.tokens.len());
        let mut cursor = start;
        while cursor < limit {
            if self.punct(cursor, "{") {
                break;
            }
            // A property declaration or a closing brace means this was never a
            // selector list.
            if self.punct(cursor, ";") || self.punct(cursor, "}") {
                return None;
            }
            cursor += 1;
        }
        if cursor >= limit || !self.punct(cursor, "{") {
            return None;
        }
        let parent = self.nesting.last().cloned().unwrap_or_default();
        let mut last_selector = String::new();
        let mut scan = start;
        while scan < cursor {
            if let Some((name, after)) = self.selector_at(scan, &parent) {
                self.facts.declarations.push(Declaration {
                    name: name.clone(),
                    kind: DeclarationKind::Selector,
                    span: self.span(scan, after.saturating_sub(1)),
                    owner: (!parent.is_empty()).then(|| parent.clone()),
                    // A stylesheet has no private selectors.
                    exported: true,
                });
                last_selector = name;
                scan = after;
                continue;
            }
            scan += 1;
        }
        self.nesting.push(if last_selector.is_empty() {
            parent
        } else {
            last_selector
        });
        Some(cursor + 1)
    }

    /// A `.class`, `#id`, or SCSS `&`-joined continuation starting at `index`.
    fn selector_at(&self, index: usize, parent: &str) -> Option<(String, usize)> {
        // `&__title` and `&--wide` extend the enclosing selector rather than
        // naming a new one, which is the form a line-based scanner cannot see.
        if self.punct(index, "&") {
            let suffix = self.text(index + 1);
            if self.kind(index + 1) == Some(TokenKind::Identifier) && !parent.is_empty() {
                return Some((format!("{parent}{suffix}"), index + 2));
            }
            return None;
        }
        let marker = if self.punct(index, ".") {
            '.'
        } else if self.punct(index, "#") {
            '#'
        } else {
            return None;
        };
        if self.kind(index + 1) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(index + 1);
        // A decimal such as `.5em` is not a class, and neither is a colour.
        if name.starts_with(|character: char| character.is_ascii_digit()) {
            return None;
        }
        Some((format!("{marker}{name}"), index + 2))
    }
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

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::DeclarationKind;
    use crate::syntax::Language;

    fn declared(source: &str, language: Language) -> Vec<String> {
        extract(source, language)
            .declarations
            .into_iter()
            .map(|item| item.name)
            .collect()
    }

    #[test]
    fn class_and_id_selectors_are_declarations() {
        let source = ".panel { color: red; }\n\
             #header, .nav .item { margin: 0; }\n\
             a:hover { color: blue; }\n";
        assert_eq!(
            declared(source, Language::Css),
            [".panel", "#header", ".nav", ".item"],
            "a pseudo-class on a bare element declares no selector"
        );
    }

    #[test]
    fn nested_scss_selectors_are_resolved_to_the_names_they_produce() {
        // The JS engine's own note admits this case is under-captured there,
        // because it has no SCSS grammar and reads CSS as flat rules.
        let source = ".card {\n\
             \x20 color: red;\n\
             \x20 &__title { font-weight: bold; }\n\
             \x20 &--wide { width: 100%; }\n\
             \x20 .inner { padding: 0; }\n\
             }\n";
        let names = declared(source, Language::Scss);
        assert!(names.contains(&".card".to_owned()), "got {names:?}");
        assert!(
            names.contains(&".card__title".to_owned()),
            "an ampersand joins the child onto its parent, got {names:?}"
        );
        assert!(names.contains(&".card--wide".to_owned()), "got {names:?}");
        assert!(names.contains(&".inner".to_owned()), "got {names:?}");
    }

    #[test]
    fn stylesheets_name_the_stylesheets_they_pull_in() {
        let source = "@import \"./base.css\";\n\
             @use \"sass:math\";\n\
             @forward \"./theme\";\n\
             .x { background: url(\"./bg.png\"); }\n";
        let imports = extract(source, Language::Scss)
            .imports
            .into_iter()
            .map(|import| (import.specifier, import.reexport))
            .collect::<Vec<_>>();
        assert_eq!(
            imports,
            [
                ("./base.css".to_owned(), false),
                ("sass:math".to_owned(), false),
                ("./theme".to_owned(), true),
            ],
            "a forward re-exports, and a url() inside a property is not an import"
        );
    }

    #[test]
    fn a_comment_declares_no_selector_and_a_decimal_is_not_a_class() {
        let source = "/* .ghost { } */\n\
             .real { margin: .5em; padding: 0 .25rem; }\n";
        assert_eq!(declared(source, Language::Css), [".real"]);
    }

    #[test]
    fn a_double_slash_is_a_comment_in_scss_and_not_in_css() {
        let scss = "// .ghost { }\n.real { }\n";
        assert_eq!(declared(scss, Language::Scss), [".real"]);
        // In plain CSS `//` is not a comment, so the selector after it on the
        // same line is still read rather than silently dropped.
        assert_eq!(
            extract("// x\n.real { }\n", Language::Css)
                .declarations
                .len(),
            1
        );
    }

    #[test]
    fn selectors_carry_the_kind_the_graph_stores_them_under() {
        let facts = extract(".only { }\n", Language::Css);
        assert_eq!(facts.declarations[0].kind, DeclarationKind::Selector);
    }
}
