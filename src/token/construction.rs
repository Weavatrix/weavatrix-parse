use super::{Language, Mode, Token, TokenKind, Tokenizer};

impl<'source> Tokenizer<'source> {
    #[must_use]
    pub fn new(source: &'source str, language: Language) -> Self {
        Self {
            source,
            syntax: language.syntax(),
            mode: Mode::Lossless,
            bytes: source.as_bytes(),
            offset: 0,
            line: 1,
            column: 1,
            value_before: false,
            at_line_start: true,
        }
    }

    /// Selects how much of the source the stream carries.
    #[must_use]
    pub const fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    pub(super) fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.offset + ahead).copied()
    }

    pub(super) fn starts_with(&self, needle: &str) -> bool {
        self.source[self.offset..].starts_with(needle)
    }

    /// Advances past `count` bytes, keeping the line and column current.
    pub(super) fn advance(&mut self, count: usize) {
        let end = (self.offset + count).min(self.bytes.len());
        while self.offset < end {
            let character = self.source[self.offset..].chars().next().unwrap_or('\0');
            if character == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
            self.offset += character.len_utf8();
        }
    }

    pub(super) fn emit(&mut self, kind: TokenKind, start: usize, line: u32, column: u32) -> Token {
        Token {
            kind,
            start,
            end: self.offset,
            line,
            column,
        }
    }
}
