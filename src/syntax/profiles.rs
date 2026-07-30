use super::{Language, Syntax};

impl Language {
    /// The lexical rules this language follows.
    #[must_use]
    pub const fn syntax(self) -> Syntax {
        match self {
            Self::JavaScript | Self::TypeScript => JAVASCRIPT,
            Self::Graphql => GRAPHQL,
            Self::Protobuf => PROTOBUF,
            Self::Rust => RUST,
            Self::Python => PYTHON,
            Self::Go | Self::Java | Self::CSharp | Self::C | Self::Cpp | Self::Solidity => C_FAMILY,
            Self::Sql => SQL,
            Self::Swift => SWIFT,
            Self::Terraform => TERRAFORM,
            Self::Html => HTML,
            Self::Css => CSS,
            Self::Scss => SCSS,
            Self::Xml => XML,
            Self::Markdown | Self::Mdx | Self::ReStructuredText | Self::AsciiDoc => PROSE,
            Self::Bash => BASH,
            Self::Yaml => YAML,
        }
    }
}

const JAVASCRIPT: Syntax = Syntax {
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
};

const GRAPHQL: Syntax = Syntax {
    line_comments: &["#"],
    block_comment: None,
    nested_block_comments: false,
    quotes: &['"'],
    interpolated_quote: None,
    escapes: true,
    regex_literals: false,
    raw_strings: false,
    // GraphQL descriptions use block strings.
    triple_quotes: true,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_'],
};

const PROTOBUF: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_block_comments: false,
    quotes: &['"', '\''],
    interpolated_quote: None,
    escapes: true,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_'],
};

const RUST: Syntax = Syntax {
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
};

const PYTHON: Syntax = Syntax {
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
};

// Solidity is lexically a C-family language; only its keywords differ.
const C_FAMILY: Syntax = Syntax {
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
};

const SQL: Syntax = Syntax {
    line_comments: &["--"],
    block_comment: Some(("/*", "*/")),
    nested_block_comments: false,
    quotes: &['\'', '"'],
    interpolated_quote: None,
    // SQL doubles a quote to escape it rather than using a backslash.
    escapes: false,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '$'],
};

const SWIFT: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    // Swift block comments nest, as Rust's do.
    nested_block_comments: true,
    quotes: &['"'],
    interpolated_quote: None,
    escapes: true,
    raw_strings: false,
    regex_literals: false,
    triple_quotes: true,
    char_literals: false,
    significant_indentation: false,
    // `$0` names a closure argument, and `_` a wildcard.
    identifier_extra: &['_', '$'],
};

const TERRAFORM: Syntax = Syntax {
    line_comments: &["#", "//"],
    block_comment: Some(("/*", "*/")),
    nested_block_comments: false,
    quotes: &['"'],
    interpolated_quote: Some('"'),
    escapes: true,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '-'],
};

const HTML: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("<!--", "-->")),
    nested_block_comments: false,
    quotes: &['"', '\''],
    interpolated_quote: None,
    // HTML escapes with entities rather than backslashes.
    escapes: false,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '-', ':', '.', '@'],
};

const CSS: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("/*", "*/")),
    nested_block_comments: false,
    quotes: &['"', '\''],
    interpolated_quote: None,
    escapes: true,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '-'],
};

const SCSS: Syntax = Syntax {
    line_comments: &["//"],
    block_comment: Some(("/*", "*/")),
    nested_block_comments: false,
    quotes: &['"', '\''],
    interpolated_quote: None,
    escapes: true,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '-', '$', '@'],
};

const XML: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("<!--", "-->")),
    nested_block_comments: false,
    quotes: &['"', '\''],
    interpolated_quote: None,
    escapes: false,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: false,
    identifier_extra: &['_', '-', ':', '.'],
};

const PROSE: Syntax = Syntax {
    line_comments: &[],
    block_comment: Some(("<!--", "-->")),
    nested_block_comments: false,
    quotes: &[],
    interpolated_quote: None,
    escapes: false,
    regex_literals: false,
    raw_strings: false,
    triple_quotes: false,
    char_literals: false,
    significant_indentation: true,
    identifier_extra: &['_', '-', '.'],
};

const BASH: Syntax = Syntax {
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
};

const YAML: Syntax = Syntax {
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
};
