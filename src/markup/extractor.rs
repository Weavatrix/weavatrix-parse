use super::{
    Extractor, Facts, Import, Language, Span, TokenKind, names_a_file, names_a_file_in_text,
    selector_use,
};

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
        // `<tag` opens an element and names the tag the following attributes
        // belong to; `</` and `>` end one.
        if self.punct(index, "<") {
            // An element whose content names a file ends its text here.
            self.close_text(index);
            if self.kind(index + 1) == Some(TokenKind::Identifier) {
                self.tag = self.text(index + 1).to_ascii_lowercase();
                return index + 2;
            }
            self.tag.clear();
            return index + 1;
        }
        if self.punct(index, ">") {
            // A script or style element holds another language, and its
            // contents are the whole point of a single-file component: a Vue
            // or Svelte file keeps its imports there and nowhere else.
            if matches!(self.tag.as_str(), "script" | "style") {
                let embedded = self.tag.clone();
                self.tag.clear();
                return self.embedded(index, &embedded);
            }
            if self.language == Language::Xml && names_a_file_in_text(&self.tag) {
                let tag = self.tag.clone();
                self.text_start = self.tokens.get(index + 1).map(|token| (token.start, tag));
            }
            self.tag.clear();
            return index + 1;
        }
        if self.tag.is_empty() {
            return index + 1;
        }
        self.attribute(index)
    }

    /// Records the text of an element whose content is a path.
    fn close_text(&mut self, index: usize) {
        let Some((start, _)) = self.text_start.take() else {
            return;
        };
        let Some(end) = self.tokens.get(index).map(|token| token.start) else {
            return;
        };
        if end <= start {
            return;
        }
        let text = self.source[start..end].trim();
        // A path has no spaces in it; prose does.
        if text.is_empty() || text.contains(char::is_whitespace) {
            return;
        }
        self.facts.imports.push(Import {
            specifier: text.to_owned(),
            span: self.span(index, index),
            type_only: false,
            reexport: false,
            names: Vec::new(),
            bindings: Vec::new(),
        });
    }

    /// Extracts the body of a `<script>` or `<style>` element with the
    /// extractor for the language it holds, and moves the facts into this
    /// file's coordinates.
    fn embedded(&mut self, close_of_open_tag: usize, tag: &str) -> usize {
        let start_token = close_of_open_tag + 1;
        let Some(start) = self.tokens.get(start_token).map(|token| token.start) else {
            return close_of_open_tag + 1;
        };
        // The body runs to the `<` that opens the closing tag.
        let mut end_token = start_token;
        while end_token < self.tokens.len() {
            if self.punct(end_token, "<")
                && self.punct(end_token + 1, "/")
                && self.text(end_token + 2).eq_ignore_ascii_case(tag)
            {
                break;
            }
            end_token += 1;
        }
        let end = self
            .tokens
            .get(end_token)
            .map_or(self.source.len(), |token| token.start);
        if end <= start {
            return end_token.max(close_of_open_tag + 1);
        }
        let body = &self.source[start..end];
        let language = if tag == "style" {
            // Everything a component writes in a style block is at least SCSS,
            // and reading plain CSS with SCSS rules costs only accepting `//`.
            Language::Scss
        } else {
            Language::TypeScript
        };
        let inner = if tag == "style" {
            crate::style::extract(body, language)
        } else {
            crate::script::extract(body, language)
        };
        let line = self.tokens[start_token].line;
        let column = self.tokens[start_token].column;
        self.absorb(inner, start, line, column);
        end_token
    }

    /// Moves facts from a fragment's coordinates into the document's.
    fn absorb(&mut self, mut inner: Facts, offset: usize, line: u32, column: u32) {
        let shift = |span: &mut Span| {
            // Only the fragment's first line shares a line with the document,
            // so only it needs the column moved.
            if span.line == 1 {
                span.column += column - 1;
            }
            if span.end_line == 1 {
                span.end_column += column - 1;
            }
            span.start += offset;
            span.end += offset;
            span.line += line - 1;
            span.end_line += line - 1;
        };
        for item in &mut inner.declarations {
            shift(&mut item.span);
        }
        for item in &mut inner.imports {
            shift(&mut item.span);
        }
        for item in &mut inner.references {
            shift(&mut item.span);
        }
        self.facts.declarations.append(&mut inner.declarations);
        self.facts.imports.append(&mut inner.imports);
        self.facts.references.append(&mut inner.references);
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
            _ if names_a_file(self.language, &self.tag, &name) => {
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
                        names: Vec::new(),
                        bindings: Vec::new(),
                    });
                }
            }
            _ => {}
        }
        value_index + 1
    }
}
