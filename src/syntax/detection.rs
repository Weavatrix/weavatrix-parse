use super::Language;

impl Language {
    /// The language a file extension selects, if this crate handles it.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension.to_ascii_lowercase().as_str() {
            "js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
            "ts" | "tsx" | "mts" | "cts" => Self::TypeScript,
            "graphql" | "gql" => Self::Graphql,
            "proto" => Self::Protobuf,
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
            Self::Graphql => "graphql",
            Self::Protobuf => "protobuf",
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
}
