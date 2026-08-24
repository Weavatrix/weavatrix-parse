use super::{AT_IMPORTS, Declaration, DeclarationKind, Extractor, Import, Span, TokenKind};

impl Extractor<'_, '_> {
    pub(super) fn run(&mut self) {
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
                        bindings: Vec::new(),
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
                let selector_span = self.span(scan, after.saturating_sub(1));
                self.facts.declarations.push(Declaration {
                    name: name.clone(),
                    kind: DeclarationKind::Selector,
                    span: selector_span,
                    extent: selector_span,
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
