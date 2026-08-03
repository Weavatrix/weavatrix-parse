//! Type-annotation uses: the dependency-injection wiring TypeScript writes
//! in constructor parameters and field annotations. No call site names these
//! types, so without them an injected provider looks unused.

use super::{Extractor, Reference, ReferenceKind, TokenKind};

impl Extractor<'_, '_> {
    /// Type uses inside a constructor's parameter list:
    /// `constructor(private orders: OrderService)`.
    pub(super) fn parameter_type_annotations(&mut self, open: usize) {
        if !self.punct(open, "(") {
            return;
        }
        let limit = (open + 128).min(self.tokens.len());
        let mut depth = 0_i32;
        let mut cursor = open;
        while cursor < limit {
            if self.punct(cursor, "(") {
                depth += 1;
            } else if self.punct(cursor, ")") {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            } else {
                self.annotated_type_use(cursor);
            }
            cursor += 1;
        }
    }

    /// Type uses in a field's annotation: `orders: OrderService;`.
    pub(super) fn field_type_annotations(&mut self, start: usize) {
        let limit = (start + 24).min(self.tokens.len());
        let mut cursor = start;
        while cursor < limit {
            if self.punct(cursor, ";")
                || self.punct(cursor, "=")
                || self.punct(cursor, ")")
                || self.punct(cursor, "}")
            {
                break;
            }
            self.annotated_type_use(cursor);
            cursor += 1;
        }
    }

    /// Records one capitalized identifier written in a type position - after
    /// the annotation colon, a generic bracket, or a union/intersection.
    fn annotated_type_use(&mut self, cursor: usize) {
        let previous = cursor.wrapping_sub(1);
        let type_position = self.punct(previous, ":")
            || self.punct(previous, "<")
            || self.punct(previous, ",")
            || self.punct(previous, "|")
            || self.punct(previous, "&");
        if !type_position
            || self.kind(cursor) != Some(TokenKind::Identifier)
            || !self.text(cursor).starts_with(char::is_uppercase)
        {
            return;
        }
        self.facts.references.push(Reference {
            name: self.text(cursor).to_owned(),
            kind: ReferenceKind::Uses,
            receiver: None,
            span: self.span(cursor, cursor),
            owner: self.owner(),
            string_arguments: Vec::new(),
            name_arguments: Vec::new(),
        });
    }
}
