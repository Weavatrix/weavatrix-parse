use super::{Facts, ParseDiagnostic, Parser, TokenKind};

impl Parser<'_> {
    pub(super) fn skip_body(&self, keyword: usize) -> Option<usize> {
        let open = self.body_open(keyword + 1)?;
        self.tokens.matching(open, "{", "}").map(|close| close + 1)
    }

    pub(super) fn body_open(&self, start: usize) -> Option<usize> {
        let mut parentheses = 0_u32;
        for index in start..self.tokens.len() {
            match self.text(index) {
                "(" => parentheses = parentheses.saturating_add(1),
                ")" => parentheses = parentheses.checked_sub(1)?,
                "{" if parentheses == 0 => return Some(index),
                "schema" | "type" | "interface" | "input" | "enum" | "scalar" | "union"
                | "query" | "mutation" | "subscription" | "fragment"
                    if parentheses == 0
                        && index > start
                        && self.text(index.saturating_sub(1)) != "@" =>
                {
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn identifier(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
    }

    pub(super) fn fail(mut self, index: usize, message: &str) -> Facts {
        self.facts = Facts::default();
        self.facts.diagnostics.push(ParseDiagnostic {
            code: "graphql.syntax_error",
            message: message.to_owned(),
            span: self.tokens.span(index),
        });
        self.facts
    }
}
