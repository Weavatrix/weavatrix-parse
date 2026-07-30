use super::{Extractor, Language, Span, TokenKind};

impl Extractor<'_, '_> {
    pub(super) fn run(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            self.close_scopes();
            index = self.step(index);
        }
    }

    pub(super) fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    pub(super) fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    pub(super) fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    pub(super) fn span(&self, start: usize, end: usize) -> Span {
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

    pub(super) fn owner(&self) -> Option<String> {
        self.scopes.last().map(|scope| scope.name.clone())
    }

    pub(super) fn test_only_at(&self, index: usize) -> bool {
        if self.language != Language::Rust {
            return false;
        }
        self.scopes.last().is_some_and(|scope| scope.test_only)
            || self.rust_test_attribute_before(index)
    }

    pub(super) fn record_test_only_declaration(&mut self, test_only: bool, span: Span) {
        if test_only {
            self.facts.test_only_declarations.push(span);
        }
    }

    /// Reads only the contiguous Rust attributes immediately before an item.
    ///
    /// The lossless tokenizer has already removed comments and strings from
    /// syntax consideration, so a quoted `#[cfg(test)]` cannot classify code.
    pub(super) fn rust_test_attribute_before(&self, index: usize) -> bool {
        let mut cursor = index;
        while cursor > 0 && self.punct(cursor - 1, "]") {
            let end = cursor - 1;
            let mut start = end;
            let mut depth = 1_i32;
            while start > 0 {
                start -= 1;
                if self.punct(start, "]") {
                    depth += 1;
                } else if self.punct(start, "[") {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            if depth != 0 || start == 0 || !self.punct(start - 1, "#") {
                break;
            }
            if self.rust_attribute_is_test(start + 1, end) {
                return true;
            }
            cursor = start - 1;
        }
        false
    }

    pub(super) fn rust_attribute_is_test(&self, start: usize, end: usize) -> bool {
        let Some(first) =
            (start..end).find(|&index| self.kind(index) == Some(TokenKind::Identifier))
        else {
            return false;
        };
        if self.text(first) == "cfg" {
            let Some(open) = (first + 1..end).find(|&index| self.punct(index, "(")) else {
                return false;
            };
            return self.cfg_has_positive_test(open + 1, end, false);
        }
        let path_end = (first..end)
            .find(|&index| self.punct(index, "("))
            .unwrap_or(end);
        (first..path_end)
            .rfind(|&index| self.kind(index) == Some(TokenKind::Identifier))
            .is_some_and(|index| {
                matches!(
                    self.text(index),
                    "test" | "rstest" | "proptest" | "wasm_bindgen_test" | "test_case"
                )
            })
    }

    pub(super) fn cfg_has_positive_test(&self, start: usize, end: usize, negated: bool) -> bool {
        let mut cursor = start;
        while cursor < end {
            if self.kind(cursor) == Some(TokenKind::Identifier) {
                if self.text(cursor) == "not" && self.punct(cursor + 1, "(") {
                    let close = self.matching_paren(cursor + 1, end);
                    if self.cfg_has_positive_test(cursor + 2, close, !negated) {
                        return true;
                    }
                    cursor = close.saturating_add(1);
                    continue;
                }
                if self.text(cursor) == "test" && !negated {
                    return true;
                }
            }
            cursor += 1;
        }
        false
    }

    pub(super) fn matching_paren(&self, open: usize, end: usize) -> usize {
        let mut depth = 0_i32;
        for cursor in open..end {
            if self.punct(cursor, "(") {
                depth += 1;
            } else if self.punct(cursor, ")") {
                depth -= 1;
                if depth == 0 {
                    return cursor;
                }
            }
        }
        end
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

    /// Discards a scope still waiting for a body when a second declaration
    /// arrives, because only one can be waiting at a time.
    ///
    /// A semicolon already does this for languages that write one. Swift and
    /// Go end a statement at the newline, so without this a `let name: String`
    /// stayed open and adopted every function declared after it.
    pub(super) fn drop_waiting(&mut self) {
        if self
            .scopes
            .last()
            .is_some_and(|scope| scope.depth.is_none())
        {
            self.scopes.pop();
        }
    }

    pub(super) fn open_body(&mut self) {
        let depth = self.depth;
        if let Some(scope) = self.scopes.last_mut()
            && scope.depth.is_none()
        {
            scope.depth = Some(depth);
        }
    }

    pub(super) fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            self.depth += 1;
            self.open_body();
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            return index + 1;
        }
        if self.punct(index, ";") {
            // A declaration whose statement ended before any brace has no
            // body, so it must not stay open and adopt what follows it -
            // which is what a Solidity `event` did to the next function.
            if self
                .scopes
                .last()
                .is_some_and(|scope| scope.depth.is_none())
            {
                self.scopes.pop();
            }
            return index + 1;
        }
        if self.kind(index) != Some(TokenKind::Identifier) {
            return index + 1;
        }
        if let Some(next) = self.import(index) {
            return next;
        }
        if let Some(next) = self.declaration(index) {
            return next;
        }
        if let Some(next) = self.call(index) {
            return next;
        }
        index + 1
    }
}
