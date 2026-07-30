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
