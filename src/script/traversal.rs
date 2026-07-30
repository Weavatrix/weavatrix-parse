use super::{
    Extractor, Facts, Scope, Span, TokenKind, extract, position_at, template_interpolation_ranges,
};

impl Extractor<'_, '_> {
    pub(super) fn run(mut self) -> Facts {
        let mut index = 0;
        while index < self.tokens.len() {
            self.close_scopes();
            index = self.step(index);
        }
        self.facts
    }

    pub(super) fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    pub(super) fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    pub(super) fn is(&self, index: usize, word: &str) -> bool {
        self.kind(index) == Some(TokenKind::Identifier) && self.text(index) == word
    }

    pub(super) fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    pub(super) fn span(&self, start: usize, end: usize) -> Span {
        let first = &self.tokens[start.min(self.tokens.len() - 1)];
        let last = &self.tokens[end.min(self.tokens.len() - 1)];
        Span {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.line,
            end_column: last.column,
        }
    }

    pub(super) fn owner(&self) -> Option<String> {
        self.scopes.last().map(|scope| scope.name.clone())
    }

    pub(super) fn close_scopes(&mut self) {
        while self
            .scopes
            .last()
            .is_some_and(|scope| scope.depth.is_some_and(|depth| self.depth < depth))
        {
            self.scopes.pop();
        }
    }

    /// Binds the innermost waiting scope to the body that just opened.
    pub(super) fn open_body(&mut self) {
        let depth = self.depth;
        if let Some(scope) = self.scopes.last_mut()
            && scope.depth.is_none()
            && scope.paren_depth == self.paren_depth
            && scope.bracket_depth == self.bracket_depth
        {
            scope.depth = Some(depth);
            scope.paren_depth = self.paren_depth;
            scope.bracket_depth = self.bracket_depth;
        }
    }

    /// Consumes one construct starting at `index`, returning the next index.
    pub(super) fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            let object_owner = self.object_literal_owner(index);
            let waiting_scope = self
                .scopes
                .last()
                .is_some_and(|scope| scope.depth.is_none());
            self.depth += 1;
            self.open_body();
            if !waiting_scope && let Some(name) = object_owner {
                self.scopes.push(Scope {
                    name,
                    depth: Some(self.depth),
                    member_body: true,
                    fields: false,
                    paren_depth: self.paren_depth,
                    bracket_depth: self.bracket_depth,
                });
            }
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            return index + 1;
        }
        if self.punct(index, "(") {
            self.paren_depth += 1;
            return index + 1;
        }
        if self.punct(index, ")") {
            self.paren_depth -= 1;
            return index + 1;
        }
        if self.punct(index, "[") {
            self.bracket_depth += 1;
            return index + 1;
        }
        if self.punct(index, "]") {
            self.bracket_depth -= 1;
            return index + 1;
        }
        if (self.is(index, "import") || self.is(index, "export"))
            && let Some(next) = self.module_statement(index)
        {
            return next;
        }
        if self.kind(index) == Some(TokenKind::String) {
            self.template_references(index);
            if let Some(next) = self.route_table(index) {
                return next;
            }
        }
        if self.kind(index) == Some(TokenKind::Identifier) {
            if let Some(next) = self.declaration(index) {
                return next;
            }
            if let Some(next) = self.call(index) {
                return next;
            }
        }
        index + 1
    }

    /// Calls inside `${...}` are program expressions even though the lossless
    /// tokenizer deliberately keeps the complete template as one string
    /// token. Extract each balanced expression separately and relocate its
    /// references to the original file. Literal template text is never parsed
    /// as code.
    pub(super) fn template_references(&mut self, index: usize) {
        let token = &self.tokens[index];
        let template = token.text(self.source);
        if !template.starts_with('`') {
            return;
        }
        let owner = self.owner();
        for (start, end) in template_interpolation_ranges(template, self.language) {
            let Some(expression) = template.get(start..end) else {
                continue;
            };
            let base = token.start + start;
            let (base_line, base_column) = position_at(self.source, base);
            let mut references = extract(expression, self.language).references;
            for reference in &mut references {
                reference.span.start += base;
                reference.span.end += base;
                reference.span.line = base_line.saturating_add(reference.span.line - 1);
                if reference.span.line == base_line {
                    reference.span.column = base_column.saturating_add(reference.span.column - 1);
                }
                reference.span.end_line = base_line.saturating_add(reference.span.end_line - 1);
                if reference.span.end_line == base_line {
                    reference.span.end_column =
                        base_column.saturating_add(reference.span.end_column - 1);
                }
                reference.owner.clone_from(&owner);
            }
            self.facts.references.extend(references);
        }
    }
}
