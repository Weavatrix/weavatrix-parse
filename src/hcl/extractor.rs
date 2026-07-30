use super::{
    DECLARING, Declaration, Extractor, Import, REFERENCE_ROOTS, Reference, ReferenceKind, Span,
    TokenKind,
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

    fn string(&self, index: usize) -> Option<String> {
        (self.kind(index) == Some(TokenKind::String))
            .then(|| self.text(index).trim_matches('"').to_owned())
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
        if self.punct(index, "{") {
            self.depth += 1;
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            self.block.pop();
            return index + 1;
        }
        if self.kind(index) != Some(TokenKind::Identifier) {
            return index + 1;
        }
        if let Some(next) = self.block_header(index) {
            return next;
        }
        if let Some(next) = self.source_attribute(index) {
            return next;
        }
        if let Some(next) = self.reference(index) {
            return next;
        }
        index + 1
    }

    /// `resource "aws_s3_bucket" "logs" {`, `module "vpc" {`, `variable "x" {`.
    fn block_header(&mut self, index: usize) -> Option<usize> {
        let word = self.text(index);
        let kind = DECLARING
            .iter()
            .find(|(name, _)| *name == word)
            .map(|(_, kind)| *kind);
        // Labels are quoted; a block may carry none, one or two of them.
        let mut cursor = index + 1;
        let mut labels = Vec::new();
        while let Some(label) = self.string(cursor) {
            labels.push(label);
            cursor += 1;
        }
        if !self.punct(cursor, "{") {
            return None;
        }
        // A resource is named by both its type and its name, which is how
        // `aws_s3_bucket.logs` is written everywhere else in the file.
        let name = labels.join(".");
        if let Some(kind) = kind
            && !name.is_empty()
        {
            let qualified = if word == "data" {
                format!("data.{name}")
            } else if word == "module" || word == "variable" {
                format!("{word}.{name}")
            } else {
                name
            };
            self.facts.declarations.push(Declaration {
                name: qualified.clone(),
                kind,
                span: self.span(index, cursor.saturating_sub(1)),
                owner: None,
                // Configuration has no private objects.
                exported: true,
            });
            self.block.push(qualified);
        } else {
            self.block.push(word.to_owned());
        }
        Some(cursor)
    }

    /// `source = "./modules/vpc"` inside a module, or a provider's registry
    /// address inside `required_providers`.
    fn source_attribute(&mut self, index: usize) -> Option<usize> {
        if self.text(index) != "source" || !self.punct(index + 1, "=") {
            return None;
        }
        let specifier = self.string(index + 2)?;
        self.facts.imports.push(Import {
            specifier,
            span: self.span(index, index + 2),
            type_only: false,
            reexport: false,
            names: Vec::new(),
            bindings: Vec::new(),
        });
        Some(index + 3)
    }

    /// `var.region`, `module.vpc.id`, `data.aws_ami.ubuntu.id`,
    /// `aws_s3_bucket.logs.arn`.
    fn reference(&mut self, index: usize) -> Option<usize> {
        if !self.punct(index + 1, ".") || self.kind(index + 2) != Some(TokenKind::Identifier) {
            return None;
        }
        let root = self.text(index);
        // A reference is either rooted at a known prefix or is a resource
        // address, which always contains an underscore in its type name.
        let rooted = REFERENCE_ROOTS.contains(&root);
        if !rooted && !root.contains('_') {
            return None;
        }
        // `var.x` names the variable; a resource address needs type and name;
        // `data.type.name` needs three.
        let parts = if root == "var" || root == "local" || root == "each" {
            2
        } else if root == "data" {
            3
        } else {
            2
        };
        let mut name = String::from(root);
        let mut cursor = index + 1;
        let mut taken = 1;
        while taken < parts && self.punct(cursor, ".") {
            if self.kind(cursor + 1) != Some(TokenKind::Identifier) {
                break;
            }
            name.push('.');
            name.push_str(self.text(cursor + 1));
            cursor += 2;
            taken += 1;
        }
        if taken < parts {
            return None;
        }
        self.facts.references.push(Reference {
            name,
            kind: ReferenceKind::Uses,
            receiver: None,
            span: self.span(index, cursor.saturating_sub(1)),
            owner: self.block.last().cloned(),
            string_arguments: Vec::new(),
            name_arguments: Vec::new(),
        });
        Some(cursor)
    }
}
