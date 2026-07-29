use crate::{Language, ParseDiagnostic, Span, Token, tokenize};
use std::ops::Index;

/// A lossless stream with a separate index for grammar-significant tokens.
pub(crate) struct ContractTokens<'source> {
    source: &'source str,
    lossless: Vec<Token>,
    structural: Vec<usize>,
}

impl<'source> ContractTokens<'source> {
    pub(crate) fn new(source: &'source str, language: Language) -> Self {
        let lossless = tokenize(source, language);
        let structural = lossless
            .iter()
            .enumerate()
            .filter_map(|(index, token)| (!token.is_trivia()).then_some(index))
            .collect();
        Self {
            source,
            lossless,
            structural,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.structural.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&Token> {
        self.structural
            .get(index)
            .and_then(|raw| self.lossless.get(*raw))
    }

    pub(crate) fn text(&self, index: usize) -> &str {
        self.get(index).map_or("", |token| token.text(self.source))
    }

    pub(crate) fn span(&self, index: usize) -> Span {
        self.get(index).map_or(
            Span {
                start: self.source.len(),
                end: self.source.len(),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            },
            |token| {
                let mut end_line = token.line;
                let mut end_column = token.column;
                for character in token.text(self.source).chars() {
                    if character == '\n' {
                        end_line = end_line.saturating_add(1);
                        end_column = 1;
                    } else {
                        end_column = end_column.saturating_add(1);
                    }
                }
                Span {
                    start: token.start,
                    end: token.end,
                    line: token.line,
                    column: token.column,
                    end_line,
                    end_column,
                }
            },
        )
    }

    pub(crate) fn matching(&self, open: usize, opening: &str, closing: &str) -> Option<usize> {
        if self.text(open) != opening {
            return None;
        }
        let mut depth = 0_u32;
        for index in open..self.len() {
            match self.text(index) {
                value if value == opening => depth = depth.saturating_add(1),
                value if value == closing => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub(crate) fn delimiter_error(&self, code: &'static str) -> Option<ParseDiagnostic> {
        let mut stack: Vec<(&str, usize)> = Vec::new();
        for index in 0..self.len() {
            match self.text(index) {
                "{" | "(" | "[" => stack.push((self.text(index), index)),
                "}" | ")" | "]" => {
                    let expected = match self.text(index) {
                        "}" => "{",
                        ")" => "(",
                        "]" => "[",
                        _ => unreachable!(),
                    };
                    match stack.pop() {
                        Some((opening, _)) if opening == expected => {}
                        _ => {
                            return Some(ParseDiagnostic {
                                code,
                                message: "unbalanced contract delimiter".to_owned(),
                                span: self.span(index),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        stack.last().map(|(_, index)| ParseDiagnostic {
            code,
            message: "unbalanced contract delimiter".to_owned(),
            span: self.span(*index),
        })
    }
}

impl Index<usize> for ContractTokens<'_> {
    type Output = Token;

    fn index(&self, index: usize) -> &Self::Output {
        &self.lossless[self.structural[index]]
    }
}
