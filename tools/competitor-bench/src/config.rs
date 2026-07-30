use std::env;

use tree_sitter::Language as TsLanguage;
use weavatrix_parse::Language;

pub(crate) type LanguageGrammar = (Language, fn() -> TsLanguage);

pub(crate) const LANGUAGE_GRAMMARS: &[LanguageGrammar] = &[
    (Language::JavaScript, || {
        tree_sitter_javascript::LANGUAGE.into()
    }),
    (Language::TypeScript, || {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    }),
    (Language::Python, || tree_sitter_python::LANGUAGE.into()),
    (Language::Rust, || tree_sitter_rust::LANGUAGE.into()),
    (Language::Go, || tree_sitter_go::LANGUAGE.into()),
    (Language::Java, || tree_sitter_java::LANGUAGE.into()),
    (Language::CSharp, || tree_sitter_c_sharp::LANGUAGE.into()),
    (Language::C, || tree_sitter_c::LANGUAGE.into()),
    (Language::Cpp, || tree_sitter_cpp::LANGUAGE.into()),
    // These are maintained community grammars. They are used for parse+walk
    // throughput only; typed contract correctness is established by the exact
    // source/span/kind fixtures, not by node counts.
    (Language::Graphql, || tree_sitter_graphql::LANGUAGE.into()),
    (Language::Protobuf, || tree_sitter_proto::LANGUAGE.into()),
    // SQL has no first-party tree-sitter grammar; this is the maintained
    // community one, which is what a consumer would actually reach for.
    (Language::Sql, || tree_sitter_sequel::LANGUAGE.into()),
    (Language::Swift, || tree_sitter_swift::LANGUAGE.into()),
    (Language::Bash, || tree_sitter_bash::LANGUAGE.into()),
    (Language::Terraform, || tree_sitter_hcl::LANGUAGE.into()),
    (Language::Markdown, || tree_sitter_md::LANGUAGE.into()),
    (Language::Xml, || tree_sitter_xml::LANGUAGE_XML.into()),
];

pub(crate) struct Config {
    pub(crate) roots: Vec<String>,
    pub(crate) output: Option<String>,
    pub(crate) auditing: bool,
    pub(crate) verifying: bool,
}

impl Config {
    pub(crate) fn from_env() -> Self {
        let mut roots = env::args().skip(1).collect::<Vec<_>>();
        let output = take_option(&mut roots, "--output");
        let auditing = roots.iter().any(|argument| argument == "--audit");
        let verifying = roots
            .iter()
            .any(|argument| argument == "--verify-ground-truth");
        roots.retain(|argument| argument != "--audit" && argument != "--verify-ground-truth");
        if roots.is_empty() {
            eprintln!(
                "usage: competitor-bench [--audit|--verify-ground-truth] \
                 [--output <path>] <corpus-dir>..."
            );
            std::process::exit(2);
        }
        Self {
            roots,
            output,
            auditing,
            verifying,
        }
    }
}

fn take_option(arguments: &mut Vec<String>, name: &str) -> Option<String> {
    let position = arguments.iter().position(|argument| argument == name)?;
    if position + 1 >= arguments.len() {
        eprintln!("{name} requires a value");
        std::process::exit(2);
    }
    arguments.remove(position);
    Some(arguments.remove(position))
}
