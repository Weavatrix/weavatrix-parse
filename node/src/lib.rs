#![deny(unsafe_op_in_unsafe_fn)]

mod view;

use napi::{Error, Result, Status};
use napi_derive::napi;
use view::{FactsView, TokenView};
use weavatrix_parse::{Facts, Language, extract, extract_path, tokenize, tokenize_lite};

#[napi(js_name = "extractFactsJson")]
pub fn extract_facts_json(source: String, language: String) -> Result<String> {
    let language = parse_language(&language)?;
    encode_facts(&extract(&source, language))
}

#[napi(js_name = "extractPathJson")]
pub fn extract_path_json(path: String, source: String) -> Result<Option<String>> {
    extract_path(&path, &source)
        .map(|facts| encode_facts(&facts))
        .transpose()
}

#[napi(js_name = "tokenizeJson")]
pub fn tokenize_json(source: String, language: String, lite: Option<bool>) -> Result<String> {
    let language = parse_language(&language)?;
    let tokens = if lite.unwrap_or(false) {
        tokenize_lite(&source, language)
    } else {
        tokenize(&source, language)
    };
    let views = tokens
        .iter()
        .map(|token| TokenView::new(token, &source))
        .collect::<Vec<_>>();
    serde_json::to_string(&views).map_err(json_error)
}

#[napi(js_name = "supportedLanguages")]
pub fn supported_languages() -> Vec<String> {
    LANGUAGES.iter().map(|value| (*value).to_owned()).collect()
}

const LANGUAGES: &[&str] = &[
    "javascript",
    "typescript",
    "graphql",
    "protobuf",
    "rust",
    "python",
    "go",
    "java",
    "csharp",
    "c",
    "cpp",
    "sql",
    "solidity",
    "swift",
    "terraform",
    "html",
    "xml",
    "markdown",
    "mdx",
    "rst",
    "asciidoc",
    "css",
    "scss",
    "bash",
    "yaml",
];

fn parse_language(value: &str) -> Result<Language> {
    let normalized = value.trim().trim_start_matches('.').to_ascii_lowercase();
    let language = match normalized.as_str() {
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
        "typescript" | "ts" | "tsx" | "mts" | "cts" => Language::TypeScript,
        "graphql" | "gql" => Language::Graphql,
        "protobuf" | "proto" => Language::Protobuf,
        "rust" | "rs" => Language::Rust,
        "python" | "py" | "pyi" => Language::Python,
        "go" => Language::Go,
        "java" => Language::Java,
        "csharp" | "cs" => Language::CSharp,
        "c" | "h" => Language::C,
        "cpp" | "c++" | "cc" | "cxx" | "hpp" => Language::Cpp,
        "sql" | "psql" => Language::Sql,
        "solidity" | "sol" => Language::Solidity,
        "swift" => Language::Swift,
        "terraform" | "tf" | "hcl" => Language::Terraform,
        "html" | "htm" => Language::Html,
        "xml" => Language::Xml,
        "markdown" | "md" => Language::Markdown,
        "mdx" => Language::Mdx,
        "rst" | "restructuredtext" => Language::ReStructuredText,
        "asciidoc" | "adoc" => Language::AsciiDoc,
        "css" => Language::Css,
        "scss" | "sass" | "less" => Language::Scss,
        "bash" | "sh" | "zsh" => Language::Bash,
        "yaml" | "yml" => Language::Yaml,
        _ => {
            return Err(Error::new(
                Status::InvalidArg,
                format!("unsupported language: {value}"),
            ));
        }
    };
    Ok(language)
}

fn encode_facts(facts: &Facts) -> Result<String> {
    serde_json::to_string(&FactsView::new(facts)).map_err(json_error)
}

fn json_error(error: serde_json::Error) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}
