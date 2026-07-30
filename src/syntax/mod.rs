//! Lexical shape of each supported language.
//!
//! Languages differ in a small number of lexical decisions - how a comment
//! starts, which quotes open a string, whether a backslash escapes, whether
//! indentation is significant - and agree on everything else. Describing those
//! differences as data keeps one tokenizer correct for all of them instead of
//! one hand-written scanner per language.

/// A language this crate can tokenize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Language {
    JavaScript,
    TypeScript,
    Graphql,
    Protobuf,
    Rust,
    Python,
    Go,
    Java,
    CSharp,
    C,
    Cpp,
    Sql,
    Solidity,
    Swift,
    Terraform,
    Html,
    Xml,
    Markdown,
    /// Markdown with JavaScript imports and components.
    Mdx,
    ReStructuredText,
    AsciiDoc,
    Css,
    /// SCSS, Sass and Less, which differ from CSS by allowing `//` comments
    /// and nesting selectors.
    Scss,
    Bash,
    Yaml,
}

mod detection;
mod profiles;

/// The lexical rules of one language.
///
/// The flags are independent lexical facts rather than a state machine, so
/// they are listed plainly instead of being packed into an option type that
/// would obscure which language has which behaviour.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
#[allow(clippy::struct_excessive_bools)]
pub struct Syntax {
    pub line_comments: &'static [&'static str],
    pub block_comment: Option<(&'static str, &'static str)>,
    pub nested_block_comments: bool,
    pub quotes: &'static [char],
    /// Quote that opens a string containing `${...}` expressions.
    pub interpolated_quote: Option<char>,
    pub escapes: bool,
    /// Whether `/` can open a regular-expression literal.
    pub regex_literals: bool,
    /// Whether `r"..."` and `r#"..."#` forms exist.
    pub raw_strings: bool,
    /// Whether `"""..."""` spans lines.
    pub triple_quotes: bool,
    /// Whether `'` opens a character literal that a lifetime is also written
    /// with. Rust needs this: `'a` is a lifetime and `'"'` is a quote
    /// character, and treating `'` as an ordinary quote or as ordinary
    /// punctuation gets one of the two wrong.
    pub char_literals: bool,
    pub significant_indentation: bool,
    pub identifier_extra: &'static [char],
}
