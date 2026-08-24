//! Structural extraction for JavaScript and TypeScript.
//!
//! The pass walks the token stream once, tracking brace depth to know which
//! declaration owns what. It reads the forms a repository graph is built from:
//! every import and re-export shape, declarations including class members,
//! and call sites with their receiver and string arguments.
//!
//! What it does not do is parse expressions. A call is recognised by an
//! identifier followed by `(`, not by building an expression tree, because no
//! consumer of these facts asks about precedence.

use crate::facts::{
    Declaration, DeclarationKind, Facts, Import, ImportBinding, Reference, ReferenceKind, Span,
};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};
use std::collections::BTreeMap;

/// Extracts structural facts from one JavaScript or TypeScript source.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    Extractor {
        source,
        tokens: &tokens,
        language,
        facts: Facts::default(),
        scopes: Vec::new(),
        import_bindings: BTreeMap::new(),
        depth: 0,
        paren_depth: 0,
        bracket_depth: 0,
    }
    .run()
}

/// A declaration whose body the walk is currently inside.
struct Scope {
    name: String,
    /// Depth of the body, once it opens. A declaration is recorded before its
    /// `{` is seen, so until then the scope is waiting and must not be closed
    /// by the very brace that opens it.
    depth: Option<i32>,
    declaration: Option<usize>,
    /// Whether members declared directly inside are class or object members.
    member_body: bool,
    /// Classes declare fields; object literals only contribute named methods.
    fields: bool,
    /// Parenthesis/bracket nesting at the member body's opening brace.
    paren_depth: i32,
    bracket_depth: i32,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
    facts: Facts,
    scopes: Vec<Scope>,
    import_bindings: BTreeMap<String, (String, bool, String)>,
    depth: i32,
    paren_depth: i32,
    bracket_depth: i32,
}

mod calls;
mod declarations;
mod modules;
mod traversal;
mod types;

/// Whether a name is an HTTP method written as a route-table key.
fn is_method(name: &str) -> bool {
    matches!(
        name,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" | "ALL"
    )
}

/// Byte ranges of the expressions enclosed by `${...}` in one JavaScript
/// template token. A nested template is one token while matching the outer
/// expression, so braces in its text cannot close the expression early.
fn template_interpolation_ranges(template: &str, language: Language) -> Vec<(usize, usize)> {
    let bytes = template.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = usize::from(bytes.first() == Some(&b'`'));
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if bytes[cursor] == b'`' {
            break;
        }
        if bytes[cursor] != b'$' || bytes[cursor + 1] != b'{' {
            cursor += 1;
            continue;
        }
        let expression_start = cursor + 2;
        let tail = &template[expression_start..];
        let tokens = Tokenizer::new(tail, language)
            .mode(Mode::Lite)
            .collect::<Vec<_>>();
        let mut depth = 1_i32;
        let mut expression_end = None;
        for token in tokens {
            if token.kind != TokenKind::Punctuation {
                continue;
            }
            match token.text(tail) {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        expression_end = Some(expression_start + token.start);
                        cursor = expression_start + token.end;
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(expression_end) = expression_end else {
            break;
        };
        ranges.push((expression_start, expression_end));
    }
    ranges
}

fn position_at(source: &str, offset: usize) -> (u32, u32) {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let column = u32::try_from(
        prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, suffix)| suffix)
            .chars()
            .count(),
    )
    .unwrap_or(u32::MAX)
    .saturating_add(1);
    (line, column)
}

#[cfg(test)]
mod tests;
