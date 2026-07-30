use super::{Extractor, Reference, ReferenceKind, TokenKind};

impl Extractor<'_, '_> {
    pub(super) fn call(&mut self, index: usize) -> Option<usize> {
        // `modelBuilder.Entity<Order>()` is a call whose parenthesis does not
        // follow the name. The type argument is what an object-relational
        // mapper names the entity with, so it is worth reaching.
        let type_arguments = self.type_argument_span(index + 1);
        let open = index + 1 + type_arguments;
        if !self.punct(open, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(
            name.as_str(),
            "if" | "for" | "while" | "switch" | "match" | "return" | "catch" | "sizeof" | "fn"
        ) {
            return None;
        }
        let receiver = (index >= 2
            && (self.punct(index - 1, ".") || self.punct(index - 1, ":"))
            && self.kind(index - 2) == Some(TokenKind::Identifier))
        .then(|| self.text(index - 2).to_owned());
        for argument in self.type_argument_names(index + 1, type_arguments) {
            self.facts.references.push(Reference {
                name: argument,
                kind: ReferenceKind::Uses,
                receiver: None,
                span: self.span(index, index),
                owner: self.owner(),
                string_arguments: Vec::new(),
                name_arguments: Vec::new(),
            });
        }
        let mut arguments = Vec::new();
        let mut scan = open + 1;
        let mut depth = 1_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if depth == 1 && self.kind(scan) == Some(TokenKind::String) {
                arguments.push(self.text(scan).trim_matches(['"', '`', '\'']).to_owned());
            }
            scan += 1;
        }
        self.facts.references.push(Reference {
            kind: ReferenceKind::Call,
            name,
            receiver,
            span: self.span(index, index),
            owner: self.owner(),
            string_arguments: arguments,
            name_arguments: Vec::new(),
        });
        Some(index + 1)
    }
}
