use super::Tokenizer;

impl Tokenizer<'_> {
    /// Consumes a string literal, returning whether it terminated.
    pub(super) fn scan_string(&mut self, quote: u8) -> bool {
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
    pub(super) fn scan_template(&mut self) -> bool {
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
    pub(super) fn scan_template_interpolation(&mut self) -> bool {
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
    pub(super) fn char_literal_length(&self) -> Option<usize> {
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
    pub(super) fn scan_raw_string(&mut self) -> bool {
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
}
