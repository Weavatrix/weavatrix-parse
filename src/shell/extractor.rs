use super::{
    CLIENTS, Declaration, DeclarationKind, Extractor, Import, KEYWORDS, RUNNERS, Reference,
    ReferenceKind, Span, TokenKind, endpoints,
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

    /// One shell word: every token written without a space between them, which
    /// is how `./lib/common.sh` and `$HOME/bin` are one argument each.
    fn word(&self, start: usize) -> (String, usize) {
        let mut text = String::new();
        let mut cursor = start;
        if start >= self.tokens.len() {
            return (text, start);
        }
        while cursor < self.tokens.len() {
            let token = &self.tokens[cursor];
            if cursor > start && self.tokens[cursor - 1].end != token.start {
                break;
            }
            if token.line != self.tokens[start].line {
                break;
            }
            let raw = token.text(self.source);
            if token.kind == TokenKind::String {
                text.push_str(raw.trim_matches(['"', '\'']));
            } else {
                text.push_str(raw);
            }
            cursor += 1;
        }
        (text, cursor)
    }

    /// Every word of the statement starting at `index`, up to its end.
    fn words(&self, start: usize) -> Vec<String> {
        let mut found = Vec::new();
        let Some(line) = self.tokens.get(start).map(|token| token.line) else {
            return found;
        };
        let mut cursor = start;
        while cursor < self.tokens.len() && self.tokens[cursor].line == line {
            // A pipe or a separator ends this command's arguments.
            if self.punct(cursor, ";") || self.punct(cursor, "|") || self.punct(cursor, "&") {
                break;
            }
            let (word, next) = self.word(cursor);
            if next == cursor {
                break;
            }
            if !word.is_empty() {
                found.push(word);
            }
            cursor = next;
        }
        found
    }

    fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            self.depth += 1;
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            if self
                .function
                .as_ref()
                .is_some_and(|(_, depth)| self.depth < *depth)
            {
                self.function = None;
            }
            return index + 1;
        }
        // `. lib.sh` and `./deploy.sh` are commands whose first character is
        // punctuation, so a word may begin with one.
        let opens_a_word = self.kind(index) == Some(TokenKind::Identifier)
            || self.punct(index, ".")
            || self.punct(index, "/");
        if !opens_a_word || !self.starts_a_statement(index) {
            return index + 1;
        }
        if let Some(next) = self.definition(index) {
            return next;
        }
        self.command(index)
    }

    /// Whether this word is the first of a command rather than an argument.
    fn starts_a_statement(&self, index: usize) -> bool {
        if index == 0 {
            return true;
        }
        let previous = &self.tokens[index - 1];
        if previous.line != self.tokens[index].line {
            return true;
        }
        // A word joined to the one before it is part of it, not a new command.
        if previous.end == self.tokens[index].start {
            return false;
        }
        matches!(previous.text(self.source), ";" | "|" | "&" | "(" | "{")
            || KEYWORDS.contains(&previous.text(self.source))
    }

    /// `function deploy {`, `deploy() {`.
    fn definition(&mut self, index: usize) -> Option<usize> {
        let (name_index, after) = if self.text(index) == "function" {
            (index + 1, index + 2)
        } else if self.punct(index + 1, "(") && self.punct(index + 2, ")") {
            (index, index + 3)
        } else {
            return None;
        };
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return None;
        }
        // `function name()` writes both forms; either way a brace follows.
        let mut cursor = after;
        while cursor < self.tokens.len() && !self.punct(cursor, "{") {
            if self.tokens[cursor].line != self.tokens[index].line {
                return None;
            }
            cursor += 1;
        }
        let name = self.text(name_index).to_owned();
        let span = self.span(index, name_index);
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Function,
            span,
            extent: span,
            owner: None,
            // A shell function is callable by anything that sources the file.
            exported: true,
        });
        self.function = Some((name, self.depth + 1));
        Some(cursor)
    }

    /// A command, which may pull in another script or address a service.
    fn command(&mut self, index: usize) -> usize {
        let (name, after) = self.word(index);
        if name.is_empty() {
            return index + 1;
        }
        let arguments = self.words(after);
        let span = self.span(index, index);

        if RUNNERS.contains(&name.as_str())
            && let Some(script) = arguments.iter().find(|word| !word.starts_with('-'))
        {
            self.facts.imports.push(Import {
                specifier: script.clone(),
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
            });
            return after;
        }
        // Running a sibling script directly is the same dependency.
        let extension = name
            .rsplit_once('.')
            .map(|(_, tail)| tail.to_ascii_lowercase());
        if matches!(extension.as_deref(), Some("sh" | "bash" | "zsh")) {
            self.facts.imports.push(Import {
                specifier: name,
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
            });
            return after;
        }

        let addresses = if CLIENTS.contains(&name.as_str()) {
            endpoints(&arguments)
        } else {
            Vec::new()
        };
        self.facts.references.push(Reference {
            name,
            kind: ReferenceKind::Call,
            receiver: None,
            span,
            owner: self.function.as_ref().map(|(name, _)| name.clone()),
            string_arguments: addresses,
            name_arguments: Vec::new(),
        });
        after
    }
}
