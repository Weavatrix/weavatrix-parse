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

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.offset + ahead).copied()
    }

    fn starts_with(&self, needle: &str) -> bool {
        self.source[self.offset..].starts_with(needle)
    }

    /// Advances past `count` bytes, keeping the line and column current.
    fn advance(&mut self, count: usize) {
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

    fn emit(&mut self, kind: TokenKind, start: usize, line: u32, column: u32) -> Token {
        Token {
            kind,
            start,
            end: self.offset,
            line,
            column,
        }
    }

    /// Consumes a string literal, returning whether it terminated.
    fn scan_string(&mut self, quote: u8) -> bool {
        if quote == b'`' {
            return self.scan_template();
        }
        let triple =
            self.syntax.triple_quotes && self.peek(1) == Some(quote) && self.peek(2) == Some(quote);
        let closing = if triple { 3 } else { 1 };
        self.advance(closing);
        loop {
            let Some(current) = self.peek(0) else {
                return false;
            };
            if self.syntax.escapes && current == b'\\' {
                self.advance(2);
                continue;
            }
            if current == quote {
                if triple {
                    if self.peek(1) == Some(quote) && self.peek(2) == Some(quote) {
                        self.advance(3);
                        return true;
                    }
                } else {
                    // SQL writes a literal quote by doubling it.
                    if !self.syntax.escapes && self.peek(1) == Some(quote) {
                        self.advance(2);
                        continue;
                    }
                    self.advance(1);
                    return true;
                }
            }
            if current == b'\n' && !triple && !self.syntax.escapes {
                return false;
            }
            self.advance(1);
        }
    }

    /// Consumes a JavaScript template, including nested templates inside its
    /// `${...}` expressions.
    ///
    /// A backtick inside an interpolation starts a nested template rather than
    /// closing the outer one. Treating it as the outer close exposed the nested
    /// template's text as code, so text such as `file(s)` became a call fact.
    fn scan_template(&mut self) -> bool {
        self.advance(1);
        loop {
            let Some(current) = self.peek(0) else {
                return false;
            };
            if current == b'\\' {
                self.advance(2);
                continue;
            }
            if current == b'`' {
                self.advance(1);
                return true;
            }
            if current == b'$' && self.peek(1) == Some(b'{') {
                self.advance(2);
                if !self.scan_template_interpolation() {
                    return false;
                }
                continue;
            }
            self.advance(1);
        }
    }

    /// Consumes the balanced expression after `${`, stopping after its `}`.
    fn scan_template_interpolation(&mut self) -> bool {
        let mut depth = 1_u32;
        loop {
            let Some(current) = self.peek(0) else {
                return false;
            };
            if matches!(current, b'\'' | b'"') {
                if !self.scan_string(current) {
                    return false;
                }
                continue;
            }
            if current == b'`' {
                if !self.scan_template() {
                    return false;
                }
                continue;
            }
            if self.starts_with("//") {
                while self.peek(0).is_some_and(|byte| byte != b'\n') {
                    self.advance(1);
                }
                continue;
            }
            if self.starts_with("/*") {
                if !self.scan_block_comment("/*", "*/") {
                    return false;
                }
                continue;
            }
            if current == b'/' {
                let start = self.offset;
                let line = self.line;
                let column = self.column;
                if self.scan_regex() {
                    continue;
                }
                // A division operator has no closing slash. Restore the
                // scanner and consume it normally.
                self.offset = start;
                self.line = line;
                self.column = column;
            }
            if current == b'{' {
                depth += 1;
            } else if current == b'}' {
                depth -= 1;
                self.advance(1);
                if depth == 0 {
                    return true;
                }
                continue;
            }
            self.advance(1);
        }
    }

    /// Length of the character literal at the cursor, or `None` when the quote
    /// opens something else.
    ///
    /// A lifetime and a character literal start identically, so what tells
    /// them apart is the closing quote: `'a'` has one and `'a` does not.
    /// Deciding by the closing quote rather than by what follows the opening
    /// one is what keeps `'"'` a literal - the case that silently shifted
    /// every string boundary in a file when `'` was treated as punctuation.
    fn char_literal_length(&self) -> Option<usize> {
        if self.peek(1) == Some(b'\\') {
            // The escaped character can itself be a quote, so the search for
            // the closing quote starts after it.
            let mut offset = 3;
            while offset < 12 {
                match self.peek(offset) {
                    Some(b'\'') => return Some(offset + 1),
                    Some(b'\n') | None => return None,
                    _ => offset += 1,
                }
            }
            return None;
        }
        let first = self.peek(1)?;
        if first == b'\'' || first == b'\n' {
            return None;
        }
        // One character, however many bytes it takes to write.
        let width = match first {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            _ => 4,
        };
        (self.peek(1 + width) == Some(b'\'')).then_some(2 + width)
    }

    /// Consumes a raw string such as `r"..."` or `r#"..."#`.
    fn scan_raw_string(&mut self) -> bool {
        let mut hashes = 0;
        self.advance(1);
        while self.peek(0) == Some(b'#') {
            hashes += 1;
            self.advance(1);
        }
        if self.peek(0) != Some(b'"') {
            return true;
        }
        self.advance(1);
        loop {
            let Some(current) = self.peek(0) else {
                return false;
            };
            if current == b'"' {
                let closes = (1..=hashes).all(|index| self.peek(index) == Some(b'#'));
                if closes {
                    self.advance(1 + hashes);
                    return true;
                }
            }
            self.advance(1);
        }
    }

    fn scan_block_comment(&mut self, open: &str, close: &str) -> bool {
        self.advance(open.len());
        let mut depth = 1_usize;
        loop {
            if self.offset >= self.bytes.len() {
                return false;
            }
            if self.starts_with(close) {
                self.advance(close.len());
                depth -= 1;
                if depth == 0 {
                    return true;
                }
                continue;
            }
            if self.syntax.nested_block_comments && self.starts_with(open) {
                self.advance(open.len());
                depth += 1;
                continue;
            }
            self.advance(1);
        }
    }

    fn scan_regex(&mut self) -> bool {
        self.advance(1);
        let mut in_class = false;
        loop {
            let Some(current) = self.peek(0) else {
                return false;
            };
            match current {
                b'\\' => {
                    self.advance(2);
                    continue;
                }
                b'\n' => return false,
                b'[' => in_class = true,
                b']' => in_class = false,
                b'/' if !in_class => {
                    self.advance(1);
                    // Trailing flags belong to the literal.
                    while self.peek(0).is_some_and(|byte| byte.is_ascii_alphabetic()) {
                        self.advance(1);
                    }
                    return true;
                }
                _ => {}
            }
            self.advance(1);
        }
    }

    fn is_identifier_start(&self, character: char) -> bool {
        character.is_alphabetic() || self.syntax.identifier_extra.contains(&character)
    }

    fn is_identifier_part(&self, character: char) -> bool {
        character.is_alphanumeric() || self.syntax.identifier_extra.contains(&character)
    }
}

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
    #[allow(clippy::too_many_lines)]
    fn scan(&mut self) -> Option<Token> {
        if self.offset >= self.bytes.len() {
            return None;
        }
        let start = self.offset;
        let line = self.line;
        let column = self.column;
        let current = self.peek(0)?;

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
        self.at_line_start = false;

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

        let character = self.source[self.offset..].chars().next()?;
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

        // A closing bracket or paren ends a value, so a following `/` divides.
        self.value_before = matches!(current, b')' | b']' | b'}');
        self.advance(character.len_utf8());
        Some(self.emit(TokenKind::Punctuation, start, line, column))
    }
}

#[cfg(test)]
mod tests {
    use super::{Token, TokenKind, tokenize};
    use crate::syntax::Language;

    /// The stream must reproduce the source byte for byte. A compiler front
    /// end and a formatter both depend on this; an extractor that silently
    /// dropped bytes could never be reused for either.
    fn assert_round_trip(source: &str, language: Language) {
        let tokens = tokenize(source, language);
        let rebuilt = tokens
            .iter()
            .map(|token| token.text(source))
            .collect::<String>();
        assert_eq!(rebuilt, source, "token stream must be lossless");
        let mut cursor = 0;
        for token in &tokens {
            assert_eq!(token.start, cursor, "tokens must be contiguous");
            assert!(token.end > token.start, "tokens must be non-empty");
            cursor = token.end;
        }
        assert_eq!(cursor, source.len(), "tokens must cover the whole source");
    }

    #[test]
    fn javascript_separates_code_from_comments_strings_and_regexes() {
        let source = "// route: app.get('/fake')\nconst re = /ab\\/c[/]/g;\nconst s = \"a // b\";\nconst t = `x ${y} z`;\napp.get('/real', h);\n";
        assert_round_trip(source, Language::JavaScript);
        let tokens = tokenize(source, Language::JavaScript);
        let strings = tokens
            .iter()
            .filter(|token| token.kind == TokenKind::String)
            .map(|token| token.text(source))
            .collect::<Vec<_>>();
        assert_eq!(strings, ["\"a // b\"", "`x ${y} z`", "'/real'"]);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::Regex)
                .map(|token| token.text(source))
                .collect::<Vec<_>>(),
            ["/ab\\/c[/]/g"],
            "a slash inside a character class does not end the literal"
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::LineComment)
                .count(),
            1,
            "the // inside a string is not a comment"
        );
    }

    #[test]
    fn division_is_not_mistaken_for_a_regex() {
        let source = "const ratio = total / count / 2;\n";
        assert_round_trip(source, Language::JavaScript);
        assert!(
            !tokenize(source, Language::JavaScript)
                .iter()
                .any(|token| token.kind == TokenKind::Regex),
            "a slash after a value divides"
        );
    }

    #[test]
    fn regex_after_return_can_hold_quotes_braces_and_backticks() {
        let source = concat!(
            "function winQuote(value) {\n",
            "  const s = String(value)\n",
            "  return /[\\s&()[\\]{}^=;!'+,`~|<>\"]/.test(s) ",
            "? `\"${s.replace(/\"/g, '\"\"')}\"` : s\n",
            "}\n",
            "export function runCommand(command, args = [], options = {}) {}\n",
        );
        assert_round_trip(source, Language::JavaScript);
        let tokens = tokenize(source, Language::JavaScript);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::Regex)
                .count(),
            1,
            "the regex inside the template interpolation belongs to its string token"
        );
        assert!(
            !tokens
                .iter()
                .any(|token| token.kind == TokenKind::Unterminated)
        );
        assert!(
            tokens.iter().any(|token| {
                token.kind == TokenKind::Identifier && token.text(source) == "runCommand"
            }),
            "the regex and template above must not swallow the next declaration"
        );
    }

    #[test]
    fn rust_block_comments_nest_and_raw_strings_hold_quotes() {
        let source = "/* outer /* inner */ still */ let s = r#\"a \"quoted\" b\"#;\n";
        assert_round_trip(source, Language::Rust);
        let tokens = tokenize(source, Language::Rust);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::BlockComment)
                .map(|token| token.text(source))
                .collect::<Vec<_>>(),
            ["/* outer /* inner */ still */"],
            "a nested comment must not end the outer one early"
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::String)
                .map(|token| token.text(source))
                .collect::<Vec<_>>(),
            ["r#\"a \"quoted\" b\"#"]
        );
    }

    #[test]
    fn python_triple_quotes_span_lines_and_indentation_is_marked() {
        let source = "def run():\n    \"\"\"doc\n    # not a comment\n    \"\"\"\n    return 1\n";
        assert_round_trip(source, Language::Python);
        let tokens = tokenize(source, Language::Python);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::String)
                .count(),
            1,
            "the docstring is one token, so the hash inside it is not a comment"
        );
        assert!(
            !tokens
                .iter()
                .any(|token| token.kind == TokenKind::LineComment),
            "no comment exists outside the docstring"
        );
        assert!(
            tokens.iter().any(|token| token.kind == TokenKind::Indent),
            "leading whitespace is marked in an indentation-sensitive language"
        );
    }

    #[test]
    fn graphql_and_protobuf_contract_sources_round_trip_losslessly() {
        let graphql = concat!(
            "\"\"\"A description with # inside\"\"\"\n",
            "type Query { user(id: ID!): User } # schema comment\n",
            "query Get($id: ID!) { user(id: $id) { id } }\n",
        );
        assert_round_trip(graphql, Language::Graphql);
        let graphql_tokens = tokenize(graphql, Language::Graphql);
        assert_eq!(
            graphql_tokens
                .iter()
                .filter(|token| token.kind == TokenKind::String)
                .count(),
            1,
            "a GraphQL block description is one lossless token"
        );
        assert_eq!(
            graphql_tokens
                .iter()
                .filter(|token| token.kind == TokenKind::LineComment)
                .count(),
            1,
            "only the hash outside the block description is a comment"
        );

        let protobuf = concat!(
            "syntax = \"proto3\";\n",
            "/* contract */ service Stream { // rpc\n",
            "  rpc Watch(stream Request) returns (stream Response);\n",
            "}\n",
        );
        assert_round_trip(protobuf, Language::Protobuf);
        let protobuf_tokens = tokenize(protobuf, Language::Protobuf);
        assert!(
            protobuf_tokens
                .iter()
                .any(|token| token.kind == TokenKind::BlockComment)
        );
        assert!(
            protobuf_tokens
                .iter()
                .any(|token| token.kind == TokenKind::LineComment)
        );
    }

    #[test]
    fn sql_doubles_quotes_to_escape_them() {
        let source = "SELECT 'it''s fine' -- trailing\nFROM users;\n";
        assert_round_trip(source, Language::Sql);
        let tokens = tokenize(source, Language::Sql);
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::String)
                .map(|token| token.text(source))
                .collect::<Vec<_>>(),
            ["'it''s fine'"]
        );
        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.kind == TokenKind::LineComment)
                .count(),
            1
        );
    }

    #[test]
    fn unterminated_constructs_are_reported_rather_than_swallowing_the_file() {
        for (source, language) in [
            ("const s = \"never closed\n", Language::JavaScript),
            ("/* never closed", Language::Rust),
        ] {
            assert_round_trip(source, language);
            assert!(
                tokenize(source, language)
                    .iter()
                    .any(|token| token.kind == TokenKind::Unterminated),
                "an unterminated construct is explicit: {source:?}"
            );
        }
    }

    #[test]
    fn lite_mode_drops_trivia_without_moving_positions() {
        let source = "// note\nconst a = 1; /* mid */ const b = 2;\n";
        let full = tokenize(source, Language::JavaScript);
        let lite = super::tokenize_lite(source, Language::JavaScript);
        assert!(
            lite.len() < full.len(),
            "the lite stream is smaller: {} vs {}",
            lite.len(),
            full.len()
        );
        assert!(
            !lite.iter().any(Token::is_trivia),
            "no trivia survives in lite mode"
        );
        let meaningful = full
            .iter()
            .filter(|token| !token.is_trivia())
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            lite, meaningful,
            "lite mode keeps the same tokens with the same spans"
        );
    }

    #[test]
    fn positions_are_one_based_and_track_lines() {
        let source = "a\nbb\n  ccc\n";
        assert_round_trip(source, Language::JavaScript);
        let tokens = tokenize(source, Language::JavaScript);
        let identifiers = tokens
            .iter()
            .filter(|token| token.kind == TokenKind::Identifier)
            .map(|token| (token.text(source), token.line, token.column))
            .collect::<Vec<_>>();
        assert_eq!(identifiers, [("a", 1, 1), ("bb", 2, 1), ("ccc", 3, 3)]);
    }
}
