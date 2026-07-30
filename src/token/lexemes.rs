use super::Tokenizer;

impl Tokenizer<'_> {
    pub(super) fn scan_block_comment(&mut self, open: &str, close: &str) -> bool {
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

    pub(super) fn scan_regex(&mut self) -> bool {
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

    pub(super) fn is_identifier_start(&self, character: char) -> bool {
        character.is_alphabetic() || self.syntax.identifier_extra.contains(&character)
    }

    pub(super) fn is_identifier_part(&self, character: char) -> bool {
        character.is_alphanumeric() || self.syntax.identifier_extra.contains(&character)
    }
}
