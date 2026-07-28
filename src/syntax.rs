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
    Rust,
    Python,
    Go,
    Java,
    CSharp,
    C,
    Cpp,
    Sql,
    Solidity,
    Bash,
    Yaml,
}

impl Language {
    /// The language a file extension selects, if this crate handles it.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "rs" => Self::Rust,
            "py" | "pyi" => Self::Python,
            "go" => Self::Go,
            "java" => Self::Java,
            "cs" => Self::CSharp,
            "c" | "h" => Self::C,
            "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => Self::Cpp,
            "sql" | "psql" => Self::Sql,
            "sol" => Self::Solidity,
            "sh" | "bash" | "zsh" => Self::Bash,
            "yaml" | "yml" => Self::Yaml,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::CSharp => "csharp",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Sql => "sql",
            Self::Solidity => "solidity",
            Self::Bash => "bash",
            Self::Yaml => "yaml",
        }
    }

    /// The lexical rules this language follows.
    #[must_use]
    pub const fn syntax(self) -> Syntax {
        match self {
            Self::JavaScript | Self::TypeScript => Syntax {
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
                nested_block_comments: false,
                quotes: &['"', '\'', '`'],
                interpolated_quote: Some('`'),
                escapes: true,
                regex_literals: true,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: false,
                identifier_extra: &['$', '_'],
            },
            Self::Rust => Syntax {
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
                // Rust block comments nest, so a naive scanner ends one early.
                nested_block_comments: true,
                quotes: &['"'],
                interpolated_quote: None,
                escapes: true,
                regex_literals: false,
                raw_strings: true,
                triple_quotes: false,
                char_literals: true,
                significant_indentation: false,
                identifier_extra: &['_'],
            },
            Self::Python => Syntax {
                line_comments: &["#"],
                block_comment: None,
                nested_block_comments: false,
                quotes: &['"', '\''],
                interpolated_quote: None,
                escapes: true,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: true,
                char_literals: false,
                significant_indentation: true,
                identifier_extra: &['_'],
            },
            // Solidity is lexically a C-family language; only its keywords
            // differ, and those live with the structural rules rather than here.
            Self::Go | Self::Java | Self::CSharp | Self::C | Self::Cpp | Self::Solidity => Syntax {
                line_comments: &["//"],
                block_comment: Some(("/*", "*/")),
                nested_block_comments: false,
                quotes: &['"', '\'', '`'],
                interpolated_quote: None,
                escapes: true,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: false,
                identifier_extra: &['_'],
            },
            Self::Sql => Syntax {
                line_comments: &["--"],
                block_comment: Some(("/*", "*/")),
                nested_block_comments: false,
                quotes: &['\'', '"'],
                interpolated_quote: None,
                // SQL doubles a quote to escape it rather than using a
                // backslash, which the tokenizer handles explicitly.
                escapes: false,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: false,
                identifier_extra: &['_', '$'],
            },
            Self::Bash => Syntax {
                line_comments: &["#"],
                block_comment: None,
                nested_block_comments: false,
                quotes: &['"', '\''],
                interpolated_quote: Some('"'),
                escapes: true,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: false,
                identifier_extra: &['_'],
            },
            Self::Yaml => Syntax {
                line_comments: &["#"],
                block_comment: None,
                nested_block_comments: false,
                quotes: &['"', '\''],
                interpolated_quote: None,
                escapes: true,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: true,
                identifier_extra: &['_', '-', '.'],
            },
        }
    }
}

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
