use weavatrix_parse::{
    ContractKind, DeclarationKind, GraphqlOperation, Language, Mode, Tokenizer, extract,
};

use crate::config::LanguageGrammar;
use crate::corpus::Corpora;

/// Reproducible correctness gates that do not pretend tree-sitter and this
/// crate expose the same facts.
///
/// The corpus gate proves byte-for-byte losslessness over every selected real
/// source. The small typed fixtures are the oracle for GraphQL, protobuf and
/// the grouped Go declaration repair: every expected fact names its exact
/// source bytes, kind and owner.
pub(crate) fn verify_ground_truth(corpora: &Corpora, languages: &[LanguageGrammar]) {
    let (files, bytes) = verify_lossless_corpus(corpora, languages);
    verify_graphql_contracts();
    verify_protobuf_contracts();
    verify_go_grouped_declarations();
    let graphql_files = corpora.get(&Language::Graphql).map_or(0, Vec::len);
    let protobuf_files = corpora.get(&Language::Protobuf).map_or(0, Vec::len);
    println!(
        "ground_truth=PASS lossless_files={files} lossless_bytes={bytes} \
         graphql_fixture=PASS graphql_corpus_files={graphql_files} \
         protobuf_fixture=PASS protobuf_corpus_files={protobuf_files} \
         go_grouped_declarations=PASS"
    );
}

fn verify_lossless_corpus(corpora: &Corpora, languages: &[LanguageGrammar]) -> (usize, usize) {
    let mut files = 0_usize;
    let mut bytes = 0_usize;
    for (language, _) in languages {
        for (path, source) in corpora.get(language).into_iter().flatten() {
            let rebuilt = Tokenizer::new(source, *language)
                .mode(Mode::Lossless)
                .map(|token| token.text(source))
                .collect::<String>();
            assert_eq!(
                rebuilt.as_bytes(),
                source.as_bytes(),
                "{} did not round-trip losslessly: {}",
                language.as_str(),
                path.display()
            );
            files += 1;
            bytes += source.len();
        }
    }
    (files, bytes)
}

fn verify_graphql_contracts() {
    let source = concat!(
        "type Query { user(id: ID!): User }\n",
        "fragment Root on Query { user { id } }\n",
        "query Get { ...Root }\n",
    );
    let facts = extract(source, Language::Graphql);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    let field = facts
        .contracts
        .iter()
        .find(|fact| {
            matches!(
                fact.kind,
                ContractKind::GraphqlField {
                    operation: Some(GraphqlOperation::Query),
                    ref return_type,
                } if return_type == "User"
            )
        })
        .expect("GraphQL query field");
    assert_eq!(field.name, "user");
    assert_eq!(field.owner.as_deref(), Some("Query"));
    assert_eq!(&source[field.span.start..field.span.end], "user");
    let spread = facts
        .contracts
        .iter()
        .find(|fact| fact.kind == ContractKind::GraphqlFragmentSpread)
        .expect("GraphQL fragment spread");
    assert_eq!(spread.name, "Root");
    assert_eq!(spread.owner.as_deref(), Some("Get"));
    assert_eq!(&source[spread.span.start..spread.span.end], "Root");
}

fn verify_protobuf_contracts() {
    let source = concat!(
        "syntax = \"proto3\";\n",
        "package shop.v1;\n",
        "import public \"common.proto\";\n",
        "message Request {}\n",
        "message Reply {}\n",
        "service Inventory {\n",
        "  rpc Watch(stream Request) returns (stream Reply);\n",
        "}\n",
    );
    let facts = extract(source, Language::Protobuf);
    assert!(facts.diagnostics.is_empty(), "{:?}", facts.diagnostics);
    assert_eq!(facts.imports.len(), 1);
    assert_eq!(facts.imports[0].specifier, "common.proto");
    assert!(facts.imports[0].reexport);
    let rpc = facts
        .contracts
        .iter()
        .find(|fact| {
            matches!(
                fact.kind,
                ContractKind::ProtobufRpc {
                    ref input,
                    ref output,
                    client_streaming: true,
                    server_streaming: true,
                } if input == "Request" && output == "Reply"
            )
        })
        .expect("streaming protobuf RPC");
    assert_eq!(rpc.name, "Watch");
    assert_eq!(rpc.owner.as_deref(), Some("Inventory"));
    assert_eq!(&source[rpc.span.start..rpc.span.end], "Watch");
}

fn verify_go_grouped_declarations() {
    let source = concat!(
        "package main\n",
        "const (\n",
        "  EventAdd = \"added\"\n",
        "  eventDelete = \"deleted\"\n",
        ")\n",
        "var (\n",
        "  endpoint = flag.String(\"endpoint\", \"/events\", \"endpoint\")\n",
        "  topics = []string{EventAdd, eventDelete}\n",
        ")\n",
    );
    let facts = extract(source, Language::Go);
    for (name, kind, line) in [
        ("EventAdd", DeclarationKind::Constant, 3),
        ("eventDelete", DeclarationKind::Constant, 4),
        ("endpoint", DeclarationKind::Variable, 7),
        ("topics", DeclarationKind::Variable, 8),
    ] {
        let fact = facts
            .declarations
            .iter()
            .find(|fact| fact.name == name && fact.kind == kind)
            .unwrap_or_else(|| panic!("missing grouped Go declaration {name}: {facts:?}"));
        assert_eq!(fact.span.line, line);
        assert_eq!(&source[fact.span.start..fact.span.end], name);
    }
}
