//! Lossless tokenizer.
//!
//! Every byte of the input belongs to exactly one token, including whitespace
//! and comments. Concatenating the text of all tokens reproduces the source
//! exactly - a property the tests assert - so the same token stream serves a
//! compiler front end, a formatter and an evidence extractor. Skipping trivia
//! would make the stream cheaper and permanently unable to round-trip.

use crate::syntax::{Language, Syntax};

/// What a token is, at the lexical level only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TokenKind {
    /// Spaces and tabs; newlines are their own kind.
    Whitespace,
    Newline,
    /// Leading whitespace of a line in an indentation-sensitive language.
    Indent,
    LineComment,
    BlockComment,
    /// String, character, template or raw-string literal, quotes included.
    String,
    /// Interpolated section of a template literal, `${` and `}` included.
    Interpolation,
    Number,
    Identifier,
    /// Regular-expression literal in languages that have them.
    Regex,
    Punctuation,
    /// A block comment or string that the file ends inside.
    Unterminated,
}

/// One lexical unit and its exact position in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Byte range in the source; `source[start..end]` is the token text.
    pub start: usize,
    pub end: usize,
    /// One-based line of the first byte.
    pub line: u32,
    /// One-based column, counted in characters, of the first byte.
    pub column: u32,
}

impl Token {
    /// The token's text.
    #[must_use]
    pub fn text<'source>(&self, source: &'source str) -> &'source str {
        source.get(self.start..self.end).unwrap_or_default()
    }

    /// Whether this token carries no program meaning.
    #[must_use]
    pub const fn is_trivia(&self) -> bool {
        matches!(
            self.kind,
            TokenKind::Whitespace
                | TokenKind::Newline
                | TokenKind::Indent
                | TokenKind::LineComment
                | TokenKind::BlockComment
        )
    }
}

/// How much of the source the stream carries.
///
/// The two modes exist because the consumers genuinely differ. A compiler
/// front end, a formatter or a source-to-source translator must be able to
/// rebuild the input, so they need every byte. Evidence extraction throws
/// trivia away immediately, so carrying it only costs allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Every byte belongs to a token; the stream rebuilds the source exactly.
    #[default]
    Lossless,
    /// Whitespace and comments are skipped. Positions stay exact, so spans
    /// remain usable, but the stream no longer round-trips.
    Lite,
}

/// Tokenizes a whole source file losslessly.
#[must_use]
pub fn tokenize(source: &str, language: Language) -> Vec<Token> {
    Tokenizer::new(source, language).collect()
}

/// Tokenizes a source file, dropping trivia.
#[must_use]
pub fn tokenize_lite(source: &str, language: Language) -> Vec<Token> {
    Tokenizer::new(source, language).mode(Mode::Lite).collect()
}

/// Streaming tokenizer over one source file.
pub struct Tokenizer<'source> {
    source: &'source str,
    syntax: Syntax,
    mode: Mode,
    bytes: &'source [u8],
    offset: usize,
    line: u32,
    column: u32,
    /// Whether the previous meaningful token can end an expression, which is
    /// what decides between division and a regular-expression literal.
    value_before: bool,
    at_line_start: bool,
}

mod construction;
mod lexemes;
mod scanner;
mod strings;

#[cfg(test)]
mod tests;
