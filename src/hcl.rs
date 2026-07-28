//! Structural extraction for Terraform and HCL.
//!
//! Infrastructure is a dependency graph that no other extractor here can see.
//! A `module` block names another directory of configuration; a `source` in
//! `required_providers` names a registry package; and every `var.x`,
//! `module.m.out` and `aws_s3_bucket.b.id` is a reference from one declared
//! object to another. Those are the same edges a code graph carries, drawn
//! over a part of the repository that has been invisible until now.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one Terraform or HCL file.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Terraform)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        block: Vec::new(),
        depth: 0,
    };
    state.run();
    state.facts
}

/// Block types whose labels name the object they declare.
const DECLARING: &[(&str, DeclarationKind)] = &[
    ("resource", DeclarationKind::Resource),
    ("data", DeclarationKind::Resource),
    ("module", DeclarationKind::Module),
    ("variable", DeclarationKind::Variable),
    ("output", DeclarationKind::Resource),
    ("provider", DeclarationKind::Module),
    ("locals", DeclarationKind::Constant),
];

/// Prefixes that introduce a reference to another declared object.
const REFERENCE_ROOTS: &[&str] = &["var", "module", "data", "local", "each"];

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    /// Names of the enclosing blocks, innermost last.
    block: Vec<String>,
    depth: i32,
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

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::DeclarationKind;

    #[test]
    fn blocks_declare_the_objects_the_rest_of_the_file_addresses() {
        let source = "resource \"aws_s3_bucket\" \"logs\" {\n\
             \x20 bucket = \"my-logs\"\n\
             }\n\
             variable \"region\" { default = \"eu-west-1\" }\n\
             data \"aws_ami\" \"ubuntu\" { most_recent = true }\n\
             output \"bucket_arn\" { value = \"x\" }\n";
        let declared = extract(source)
            .declarations
            .into_iter()
            .map(|item| (item.name, item.kind))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                ("aws_s3_bucket.logs".to_owned(), DeclarationKind::Resource),
                ("variable.region".to_owned(), DeclarationKind::Variable),
                ("data.aws_ami.ubuntu".to_owned(), DeclarationKind::Resource),
                ("bucket_arn".to_owned(), DeclarationKind::Resource),
            ],
            "a resource is addressed by type and name together"
        );
    }

    #[test]
    fn a_module_names_the_configuration_it_pulls_in() {
        let source = "module \"vpc\" {\n\
             \x20 source  = \"./modules/vpc\"\n\
             \x20 version = \"1.2.0\"\n\
             }\n\
             terraform {\n\
             \x20 required_providers {\n\
             \x20   aws = { source = \"hashicorp/aws\" }\n\
             \x20 }\n\
             }\n";
        let facts = extract(source);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["./modules/vpc", "hashicorp/aws"],
            "a local module and a registry provider are both dependencies"
        );
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.name == "module.vpc")
        );
    }

    #[test]
    fn interpolations_reference_the_objects_they_name() {
        let source = "resource \"aws_instance\" \"web\" {\n\
             \x20 ami           = data.aws_ami.ubuntu.id\n\
             \x20 subnet_id     = module.vpc.public_subnet\n\
             \x20 instance_type = var.instance_type\n\
             \x20 bucket        = aws_s3_bucket.logs.arn\n\
             }\n";
        let used = extract(source)
            .references
            .into_iter()
            .map(|reference| (reference.name, reference.owner))
            .collect::<Vec<_>>();
        assert_eq!(
            used,
            [
                (
                    "data.aws_ami.ubuntu".to_owned(),
                    Some("aws_instance.web".to_owned())
                ),
                ("module.vpc".to_owned(), Some("aws_instance.web".to_owned())),
                (
                    "var.instance_type".to_owned(),
                    Some("aws_instance.web".to_owned())
                ),
                (
                    "aws_s3_bucket.logs".to_owned(),
                    Some("aws_instance.web".to_owned())
                ),
            ],
            "every reference belongs to the resource that makes it"
        );
    }

    #[test]
    fn a_comment_declares_nothing() {
        let source = "# resource \"aws_s3_bucket\" \"ghost\" {}\n\
             // module \"ghost\" { source = \"./nowhere\" }\n\
             /* variable \"ghost\" {} */\n\
             resource \"aws_vpc\" \"real\" {}\n";
        let facts = extract(source);
        assert_eq!(facts.declarations.len(), 1);
        assert!(facts.imports.is_empty());
    }
}
