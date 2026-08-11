use super::{
    Declaration, DeclarationKind, Extractor, Import, ImportBinding, Reference, ReferenceKind,
    Scope, Span, TokenKind,
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

    fn span_offsets(&self, start: usize, end: usize) -> Span {
        let position = |offset: usize| {
            let prefix = &self.source[..offset.min(self.source.len())];
            let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1)
                .unwrap_or(u32::MAX);
            let column = u32::try_from(
                prefix
                    .rsplit_once('\n')
                    .map_or(prefix, |(_, tail)| tail)
                    .chars()
                    .count()
                    + 1,
            )
            .unwrap_or(u32::MAX);
            (line, column)
        };
        let (line, column) = position(start);
        let (end_line, end_column) = position(end);
        Span {
            start,
            end,
            line,
            column,
            end_line,
            end_column,
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
        if self.kind(index) == Some(TokenKind::String) {
            self.f_string_calls(index);
            return index + 1;
        }
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

    fn f_string_calls(&mut self, index: usize) {
        let is_f_string = index
            .checked_sub(1)
            .filter(|previous| self.tokens[*previous].end == self.tokens[index].start)
            .filter(|previous| self.kind(*previous) == Some(TokenKind::Identifier))
            .map(|previous| self.text(previous).to_ascii_lowercase())
            .is_some_and(|prefix| {
                prefix.contains('f')
                    && prefix
                        .chars()
                        .all(|character| matches!(character, 'f' | 'r' | 'b' | 'u'))
            });
        if !is_f_string {
            return;
        }
        let token = &self.tokens[index];
        let text = token.text(self.source);
        let owner = self.owner();
        let mut references = Vec::new();
        for (start, end) in f_string_sections(text) {
            let Some(expression) = text.get(start..end) else {
                continue;
            };
            for mut reference in super::extract(expression).references {
                let base = token.start + start;
                reference.span =
                    self.span_offsets(base + reference.span.start, base + reference.span.end);
                reference.owner.clone_from(&owner);
                references.push(reference);
            }
        }
        self.facts.references.extend(references);
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
        // `class Service(Base, Mixin):` names what it derives from, and those
        // are the edges an architecture rule reasons about.
        if kind == DeclarationKind::Class && self.punct(name_index + 1, "(") {
            let limit = (name_index + 64).min(self.tokens.len());
            let mut cursor = name_index + 2;
            while cursor < limit && !self.punct(cursor, ")") {
                if self.kind(cursor) == Some(TokenKind::Identifier)
                    && !self.punct(cursor + 1, "=")
                    && !self.punct(cursor.wrapping_sub(1), ".")
                {
                    self.facts.references.push(Reference {
                        name: self.text(cursor).to_owned(),
                        kind: ReferenceKind::Inherits,
                        receiver: None,
                        span: self.span(cursor, cursor),
                        owner: Some(name.clone()),
                        string_arguments: Vec::new(),
                        name_arguments: Vec::new(),
                    });
                }
                cursor += 1;
            }
        }
        self.scopes.push(Scope { name, column });
        name_index + 1
    }

    /// `import a.b`, `import a as b`, `from .pkg import x`, `from x import *`.
    fn import(&mut self, index: usize) -> Option<usize> {
        let from_form = self.is(index, "from");
        let line = self.tokens[index].line;
        let mut cursor = index + 1;
        if from_form {
            let mut specifier = String::new();
            while cursor < self.tokens.len()
                && self.tokens[cursor].line == line
                && !self.is(cursor, "import")
            {
                let text = self.text(cursor);
                if text == "." || self.kind(cursor) == Some(TokenKind::Identifier) {
                    specifier.push_str(text);
                }
                cursor += 1;
            }
            if specifier.is_empty() || !self.is(cursor, "import") {
                return None;
            }
            cursor += 1;
            let (bindings, end) = self.python_bindings(cursor, line);
            self.push_import(&specifier, index, end.saturating_sub(1), bindings);
            return Some(end);
        }

        while cursor < self.tokens.len() && self.tokens[cursor].line == line {
            let start = cursor;
            let mut specifier = String::new();
            while cursor < self.tokens.len()
                && self.tokens[cursor].line == line
                && !self.punct(cursor, ",")
                && !self.is(cursor, "as")
            {
                let text = self.text(cursor);
                if text == "." || self.kind(cursor) == Some(TokenKind::Identifier) {
                    specifier.push_str(text);
                }
                cursor += 1;
            }
            if specifier.is_empty() {
                return None;
            }
            let mut local = specifier
                .split('.')
                .next()
                .unwrap_or(specifier.as_str())
                .to_owned();
            if self.is(cursor, "as") && self.kind(cursor + 1) == Some(TokenKind::Identifier) {
                self.text(cursor + 1).clone_into(&mut local);
                cursor += 2;
            }
            self.push_import(
                &specifier,
                start,
                cursor.saturating_sub(1),
                vec![ImportBinding {
                    imported: specifier.clone(),
                    local,
                }],
            );
            if self.punct(cursor, ",") {
                cursor += 1;
            }
        }
        Some(cursor)
    }

    fn python_bindings(&self, start: usize, line: u32) -> (Vec<ImportBinding>, usize) {
        let mut bindings = Vec::new();
        let mut cursor = start;
        while cursor < self.tokens.len() && self.tokens[cursor].line == line {
            if self.kind(cursor) != Some(TokenKind::Identifier) {
                cursor += 1;
                continue;
            }
            let imported = self.text(cursor).to_owned();
            let mut local = imported.clone();
            if self.is(cursor + 1, "as") && self.kind(cursor + 2) == Some(TokenKind::Identifier) {
                self.text(cursor + 2).clone_into(&mut local);
                cursor += 3;
            } else {
                cursor += 1;
            }
            bindings.push(ImportBinding { imported, local });
        }
        (bindings, cursor)
    }

    fn push_import(
        &mut self,
        specifier: &str,
        start: usize,
        end: usize,
        bindings: Vec<ImportBinding>,
    ) {
        let names = bindings
            .iter()
            .map(|binding| binding.local.clone())
            .collect();
        self.facts.imports.push(Import {
            specifier: specifier.to_owned(),
            span: self.span(start, end),
            type_only: false,
            reexport: false,
            names,
            bindings,
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
            name_arguments: Vec::new(),
        });
        Some(index + 1)
    }
}

/// Executable `{...}` sections of a Python f-string token.
///
/// Doubled braces are literal text. Quotes and escapes inside an expression are tracked so a
/// dictionary or string literal cannot close the section early.
fn f_string_sections(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut sections = Vec::new();
    let mut at = 0_usize;
    while at < bytes.len() {
        if bytes[at] != b'{' {
            at += 1;
            continue;
        }
        if bytes.get(at + 1) == Some(&b'{') {
            at += 2;
            continue;
        }
        let start = at + 1;
        let mut cursor = start;
        let mut depth = 1_u32;
        let mut quote = None;
        let mut escaped = false;
        while cursor < bytes.len() {
            let byte = bytes[cursor];
            if let Some(delimiter) = quote {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == delimiter {
                    quote = None;
                }
                cursor += 1;
                continue;
            }
            match byte {
                b'\'' | b'"' => quote = Some(byte),
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        sections.push((start, cursor));
                        cursor += 1;
                        break;
                    }
                }
                _ => {}
            }
            cursor += 1;
        }
        at = cursor.max(at + 1);
    }
    sections
}
