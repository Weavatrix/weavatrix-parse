use super::{Mode, Token, TokenKind, Tokenizer};

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let token = self.scan()?;
            if self.mode == Mode::Lite && token.is_trivia() {
                continue;
            }
            return Some(token);
        }
    }
}

impl Tokenizer<'_> {
    pub(super) fn scan(&mut self) -> Option<Token> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let start = self.offset;
        let line = self.line;
        let column = self.column;
        let current = self.peek(0)?;

        if let Some(token) = self.scan_layout(current, start, line, column) {
            return Some(token);
        }
        self.at_line_start = false;
        if let Some(token) = self.scan_comment(start, line, column) {
            return Some(token);
        }
        if let Some(token) = self.scan_literal(current, start, line, column) {
            return Some(token);
        }
        if let Some(token) = self.scan_number(current, start, line, column) {
            return Some(token);
        }

        let character = self.source[self.offset..].chars().next()?;
        if let Some(token) = self.scan_identifier(character, start, line, column) {
            return Some(token);
        }

        // A closing bracket or paren ends a value, so a following `/` divides.
        self.value_before = matches!(current, b')' | b']' | b'}');
        self.advance(character.len_utf8());
        Some(self.emit(TokenKind::Punctuation, start, line, column))
    }

    fn scan_layout(&mut self, current: u8, start: usize, line: u32, column: u32) -> Option<Token> {
        if current == b'\n' || (current == b'\r' && self.peek(1) == Some(b'\n')) {
            self.advance(if current == b'\r' { 2 } else { 1 });
            self.value_before = false;
            self.at_line_start = true;
            return Some(self.emit(TokenKind::Newline, start, line, column));
        }

        if current == b' ' || current == b'\t' || current == b'\r' {
            let indent = self.at_line_start && self.syntax.significant_indentation;
            while matches!(self.peek(0), Some(b' ' | b'\t' | b'\r')) {
                self.advance(1);
            }
            self.at_line_start = false;
            let kind = if indent {
                TokenKind::Indent
            } else {
                TokenKind::Whitespace
            };
            return Some(self.emit(kind, start, line, column));
        }
        None
    }

    fn scan_comment(&mut self, start: usize, line: u32, column: u32) -> Option<Token> {
        for marker in self.syntax.line_comments {
            if self.starts_with(marker) {
                while !matches!(self.peek(0), None | Some(b'\n')) {
                    self.advance(1);
                }
                return Some(self.emit(TokenKind::LineComment, start, line, column));
            }
        }

        if let Some((open, close)) = self.syntax.block_comment
            && self.starts_with(open)
        {
            let terminated = self.scan_block_comment(open, close);
            let kind = if terminated {
                TokenKind::BlockComment
            } else {
                TokenKind::Unterminated
            };
            return Some(self.emit(kind, start, line, column));
        }
        None
    }

    fn scan_literal(&mut self, current: u8, start: usize, line: u32, column: u32) -> Option<Token> {
        if self.syntax.raw_strings
            && (current == b'r' || current == b'b')
            && matches!(self.peek(1), Some(b'"' | b'#'))
        {
            let terminated = self.scan_raw_string();
            let kind = if terminated {
                TokenKind::String
            } else {
                TokenKind::Unterminated
            };
            self.value_before = true;
            return Some(self.emit(kind, start, line, column));
        }

        if self.syntax.char_literals
            && current == b'\''
            && let Some(length) = self.char_literal_length()
        {
            self.advance(length);
            self.value_before = true;
            return Some(self.emit(TokenKind::String, start, line, column));
        }

        if self.syntax.quotes.contains(&(current as char)) {
            let terminated = self.scan_string(current);
            let kind = if terminated {
                TokenKind::String
            } else {
                TokenKind::Unterminated
            };
            self.value_before = true;
            return Some(self.emit(kind, start, line, column));
        }

        if self.syntax.regex_literals && current == b'/' && !self.value_before {
            let terminated = self.scan_regex();
            if terminated {
                self.value_before = true;
                return Some(self.emit(TokenKind::Regex, start, line, column));
            }
            // Not a regular expression after all: fall through as punctuation.
            self.offset = start;
            self.line = line;
            self.column = column;
        }
        None
    }

    fn scan_number(&mut self, current: u8, start: usize, line: u32, column: u32) -> Option<Token> {
        if current.is_ascii_digit() {
            while self
                .peek(0)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'_')
            {
                self.advance(1);
            }
            self.value_before = true;
            return Some(self.emit(TokenKind::Number, start, line, column));
        }
        None
    }

    fn scan_identifier(
        &mut self,
        character: char,
        start: usize,
        line: u32,
        column: u32,
    ) -> Option<Token> {
        if self.is_identifier_start(character) {
            while self.source[self.offset..]
                .chars()
                .next()
                .is_some_and(|value| self.is_identifier_part(value))
            {
                let width = self.source[self.offset..]
                    .chars()
                    .next()
                    .map_or(1, char::len_utf8);
                self.advance(width);
            }
            let identifier = &self.source[start..self.offset];
            self.value_before = !self.syntax.regex_literals
                || !matches!(
                    identifier,
                    "await"
                        | "case"
                        | "delete"
                        | "do"
                        | "else"
                        | "in"
                        | "instanceof"
                        | "new"
                        | "of"
                        | "return"
                        | "throw"
                        | "typeof"
                        | "void"
                        | "yield"
                );
            return Some(self.emit(TokenKind::Identifier, start, line, column));
        }
        None
    }
}
