//! Structural extraction for Python.
//!
//! Python scopes by indentation rather than braces, so the walk tracks the
//! column a declaration was written at and closes it when a later declaration
//! appears at the same column or further left. Working from token columns
//! rather than raw line prefixes keeps this correct inside triple-quoted
//! strings, where a line that looks like `def x():` is text, not code.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one Python source.
#[must_use]
pub fn extract(source: &str) -> Facts {
    let tokens = Tokenizer::new(source, Language::Python)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        scopes: Vec::new(),
    };
    state.run();
    state.facts
}

/// A `def` or `class` whose indented body the walk is inside.
struct Scope {
    name: String,
    column: u32,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    scopes: Vec<Scope>,
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

    fn is(&self, index: usize, word: &str) -> bool {
        self.kind(index) == Some(TokenKind::Identifier) && self.text(index) == word
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

    /// Closes every scope this column has left.
    fn close_scopes(&mut self, column: u32) {
        while self
            .scopes
            .last()
            .is_some_and(|scope| column <= scope.column)
        {
            self.scopes.pop();
        }
    }

    fn owner(&self) -> Option<String> {
        self.scopes.last().map(|scope| scope.name.clone())
    }

    fn step(&mut self, index: usize) -> usize {
        let column = self.tokens[index].column;
        if self.kind(index) != Some(TokenKind::Identifier) {
            return index + 1;
        }
        if self.is(index, "def") || self.is(index, "async") && self.is(index + 1, "def") {
            let keyword = if self.is(index, "async") {
                index + 1
            } else {
                index
            };
            return self.definition(index, keyword + 1, DeclarationKind::Function, column);
        }
        if self.is(index, "class") {
            return self.definition(index, index + 1, DeclarationKind::Class, column);
        }
        if (self.is(index, "import") || self.is(index, "from"))
            && let Some(next) = self.import(index)
        {
            return next;
        }
        if let Some(next) = self.call(index) {
            return next;
        }
        index + 1
    }

    fn definition(
        &mut self,
        start: usize,
        name_index: usize,
        kind: DeclarationKind,
        column: u32,
    ) -> usize {
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return start + 1;
        }
        self.close_scopes(column);
        let name = self.text(name_index).to_owned();
        // A def written inside a class is a method of it.
        let kind = if kind == DeclarationKind::Function && self.owner().is_some() {
            DeclarationKind::Method
        } else {
            kind
        };
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind,
            span: self.span(start, name_index),
            owner: self.owner(),
            // Python exports by convention: a leading underscore is private.
            exported: !name.starts_with('_'),
        });
        self.scopes.push(Scope { name, column });
        name_index + 1
    }

    /// `import a.b`, `import a as b`, `from .pkg import x`, `from x import *`.
    fn import(&mut self, index: usize) -> Option<usize> {
        let from_form = self.is(index, "from");
        let mut cursor = index + 1;
        let mut specifier = String::new();
        while cursor < self.tokens.len() {
            let token = &self.tokens[cursor];
            if token.line != self.tokens[index].line {
                break;
            }
            if from_form && self.is(cursor, "import") {
                break;
            }
            if !from_form && self.punct(cursor, ",") {
                // `import a, b` declares two modules; record and continue.
                if !specifier.is_empty() {
                    self.push_import(&specifier, index, cursor);
                    specifier.clear();
                }
                cursor += 1;
                continue;
            }
            if self.is(cursor, "as") {
                // The alias is not part of the module path.
                while cursor < self.tokens.len()
                    && self.tokens[cursor].line == self.tokens[index].line
                    && !self.punct(cursor, ",")
                {
                    cursor += 1;
                }
                continue;
            }
            if matches!(
                self.kind(cursor),
                Some(TokenKind::Identifier | TokenKind::Punctuation)
            ) {
                let text = self.text(cursor);
                if text == "." || self.kind(cursor) == Some(TokenKind::Identifier) {
                    specifier.push_str(text);
                }
            }
            cursor += 1;
        }
        if specifier.is_empty() {
            return None;
        }
        self.push_import(&specifier, index, cursor.saturating_sub(1));
        // In `from x import a, b` the names after `import` are bindings, not
        // modules; skipping to the end of the line stops them from being read
        // as another import statement.
        let line = self.tokens[index].line;
        while cursor < self.tokens.len() && self.tokens[cursor].line == line {
            cursor += 1;
        }
        Some(cursor)
    }

    fn push_import(&mut self, specifier: &str, start: usize, end: usize) {
        self.facts.imports.push(Import {
            specifier: specifier.to_owned(),
            span: self.span(start, end),
            type_only: false,
            reexport: false,
        });
    }

    fn call(&mut self, index: usize) -> Option<usize> {
        if !self.punct(index + 1, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(
            name.as_str(),
            "if" | "while" | "for" | "return" | "print" | "def" | "class" | "except" | "with"
        ) {
            return None;
        }
        let receiver = (index >= 2
            && self.punct(index - 1, ".")
            && self.kind(index - 2) == Some(TokenKind::Identifier))
        .then(|| self.text(index - 2).to_owned());
        let mut arguments = Vec::new();
        let mut scan = index + 2;
        let mut depth = 1_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if depth == 1 && self.kind(scan) == Some(TokenKind::String) {
                let raw = self.text(scan);
                let trimmed = raw
                    .trim_start_matches(['"', '\''])
                    .trim_end_matches(['"', '\'']);
                arguments.push(trimmed.to_owned());
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
        });
        Some(index + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::DeclarationKind;

    #[test]
    fn methods_belong_to_their_class_and_indentation_closes_scopes() {
        let source = "class Service:\n\
             \x20   def run(self):\n\
             \x20       return self.helper()\n\
             \x20   def helper(self):\n\
             \x20       return 1\n\
             \n\
             def module_level():\n\
             \x20   return Service()\n";
        let facts = extract(source);
        let declared = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                ("Service", DeclarationKind::Class, None),
                ("run", DeclarationKind::Method, Some("Service")),
                ("helper", DeclarationKind::Method, Some("Service")),
                ("module_level", DeclarationKind::Function, None),
            ],
            "dedenting to column one leaves the class"
        );
    }

    #[test]
    fn a_docstring_is_text_even_when_it_looks_like_code() {
        let source = "def real():\n\
             \x20   \"\"\"\n\
             \x20   def fake():\n\
             \x20       import nothing\n\
             \x20   \"\"\"\n\
             \x20   return 1\n";
        let facts = extract(source);
        assert_eq!(
            facts
                .declarations
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
            ["real"],
            "the definition inside the docstring is not a declaration"
        );
        assert!(
            facts.imports.is_empty(),
            "the import inside the docstring is not a dependency"
        );
    }

    #[test]
    fn reads_the_import_forms_python_writes() {
        let source = "import os\n\
             import pkg.module\n\
             import json, time\n\
             import numpy as np\n\
             from .relative import thing\n\
             from ..parent.pkg import other\n";
        let specifiers = extract(source)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect::<Vec<_>>();
        assert_eq!(
            specifiers,
            [
                "os",
                "pkg.module",
                "json",
                "time",
                "numpy",
                ".relative",
                "..parent.pkg",
            ]
        );
    }

    #[test]
    fn underscore_names_are_not_exported() {
        let facts = extract("def public():\n    pass\ndef _private():\n    pass\n");
        let exported = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.exported))
            .collect::<Vec<_>>();
        assert_eq!(exported, [("public", true), ("_private", false)]);
    }
}
