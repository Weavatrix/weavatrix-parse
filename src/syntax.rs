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
            "swift" => Self::Swift,
            "tf" | "tfvars" | "hcl" => Self::Terraform,
            "html" | "htm" | "xhtml" | "vue" | "svelte" => Self::Html,
            "xml" | "xsd" | "xsl" | "xslt" | "pom" | "csproj" | "vbproj" | "fsproj" | "props"
            | "targets" | "plist" | "storyboard" | "xib" | "resx" | "nuspec" => Self::Xml,
            "md" | "markdown" | "mdown" | "mkd" | "mkdn" => Self::Markdown,
            "mdx" => Self::Mdx,
            "rst" => Self::ReStructuredText,
            "adoc" | "asciidoc" | "asc" => Self::AsciiDoc,
            "css" => Self::Css,
            "scss" | "sass" | "less" => Self::Scss,
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
            Self::Swift => "swift",
            Self::Terraform => "terraform",
            Self::Xml => "xml",
            Self::Markdown => "markdown",
            Self::Mdx => "mdx",
            Self::ReStructuredText => "rst",
            Self::AsciiDoc => "asciidoc",
            Self::Html => "html",
            Self::Css => "css",
            Self::Scss => "scss",
            Self::Bash => "bash",
            Self::Yaml => "yaml",
        }
    }

    /// The lexical rules this language follows.
    // This is a table, not an algorithm: one arm per language, each a literal.
    // Splitting it to satisfy a line count would scatter the table across
    // functions and make the languages harder to compare against each other,
    // which is the whole point of writing them as data.
    #[allow(clippy::too_many_lines)]
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
            Self::Swift => Syntax {
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
            },
            Self::Terraform => Syntax {
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
            },
            // A tag is punctuation and identifiers around a quoted value, so
            // the ordinary token model fits once `-` and `:` are allowed in a
            // name: `data-count`, `aria-label`, `xlink:href`, `v-on:click`.
            Self::Html => Syntax {
                line_comments: &[],
                block_comment: Some(("<!--", "-->")),
                nested_block_comments: false,
                quotes: &['"', '\''],
                interpolated_quote: None,
                // HTML escapes with entities rather than backslashes, so a
                // backslash before a quote does not extend the value.
                escapes: false,
                regex_literals: false,
                raw_strings: false,
                triple_quotes: false,
                char_literals: false,
                significant_indentation: false,
                identifier_extra: &['_', '-', ':', '.', '@'],
            },
            // CSS has no line comment: `//` is invalid there, and treating it
            // as one would swallow the rest of a line in a valid stylesheet.
            Self::Css => Syntax {
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
            },
            Self::Scss => Syntax {
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
            },
            // XML shares HTML's shape; what differs is which attributes name a
            // file, and that belongs with the extractor rather than here.
            Self::Xml => Syntax {
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
            },
            // Prose has no token structure worth the name: a `"` is a quotation
            // mark, not a literal, and `//` is part of a URL. These rules exist
            // so the tokenizer stays total over every language; the document
            // extractors read lines directly, which is the honest model.
            Self::Markdown | Self::Mdx | Self::ReStructuredText | Self::AsciiDoc => Syntax {
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
