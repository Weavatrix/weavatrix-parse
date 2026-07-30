use super::{Extractor, Import, ImportBinding, Reference, ReferenceKind, TokenKind};

impl Extractor<'_, '_> {
    /// A call site, with the receiver it was written on.
    pub(super) fn call(&mut self, index: usize) -> Option<usize> {
        let type_arguments = self.type_argument_span(index + 1);
        let open = index + 1 + type_arguments;
        if !self.punct(open, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(
            name.as_str(),
            "if" | "for" | "while" | "switch" | "catch" | "function" | "return" | "typeof"
        ) {
            return None;
        }
        let receiver = (self.punct(index.wrapping_sub(1), ".")
            && self.kind(index.wrapping_sub(2)) == Some(TokenKind::Identifier))
        .then(|| self.text(index - 2).to_owned());
        let mut arguments = Vec::new();
        let mut names = Vec::new();
        let mut scan = open + 1;
        let mut depth = 1_i32;
        let mut nested = 0_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if self.punct(scan, "{") || self.punct(scan, "[") {
                nested += 1;
            } else if self.punct(scan, "}") || self.punct(scan, "]") {
                nested -= 1;
            } else if depth == 1
                && nested == 0
                && self.kind(scan) == Some(TokenKind::String)
                && let Some(value) = self.string_at(scan)
            {
                arguments.push(value);
            } else if depth == 1 && nested == 0 && self.kind(scan) == Some(TokenKind::Identifier) {
                // A bare name passed as an argument: the router in
                // `app.use("/api", router)`, the handler in `app.get(path, h)`.
                // A member access contributes only its root, because that is
                // the binding an importer can resolve.
                if !self.punct(scan.wrapping_sub(1), ".") {
                    names.push(self.text(scan).to_owned());
                }
            }
            scan += 1;
        }
        // `require('x')` is how CommonJS imports, so it is an import as well
        // as a call; recording only the call would lose the dependency.
        if name == "require"
            && receiver.is_none()
            && let Some(specifier) = arguments.first()
        {
            // `const router = require('./x')` binds the module to a name, and
            // a mount written later refers to it by that name and nothing
            // else.
            let bound = if self.punct(index.wrapping_sub(1), "=")
                && self.kind(index.wrapping_sub(2)) == Some(TokenKind::Identifier)
            {
                vec![self.text(index - 2).to_owned()]
            } else {
                Vec::new()
            };
            self.facts.imports.push(Import {
                specifier: specifier.clone(),
                span: self.span(index, index),
                type_only: false,
                reexport: false,
                bindings: bound
                    .iter()
                    .map(|local| ImportBinding {
                        imported: "*".to_owned(),
                        local: local.clone(),
                    })
                    .collect(),
                names: bound,
            });
        }
        self.facts.references.push(Reference {
            kind: ReferenceKind::Call,
            name,
            receiver,
            span: self.span(index, index),
            owner: self.owner(),
            string_arguments: arguments,
            name_arguments: names,
        });
        Some(index + 1)
    }

    /// Length of a balanced TypeScript type-argument list before a call.
    pub(super) fn type_argument_span(&self, index: usize) -> usize {
        if !self.punct(index, "<") {
            return 0;
        }
        let limit = (index + 64).min(self.tokens.len());
        let mut cursor = index + 1;
        let mut depth = 1_i32;
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "<") {
                depth += 1;
            } else if self.punct(cursor, ">") {
                depth -= 1;
            } else if depth == 1 && matches!(self.text(cursor), ";" | "{" | "}") {
                return 0;
            }
            cursor += 1;
        }
        if depth == 0 && self.punct(cursor, "(") {
            cursor - index
        } else {
            0
        }
    }
}
