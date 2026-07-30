use super::*;
use crate::facts::ContractKind;

#[test]
fn extracts_typed_schema_operations_fragments_and_exact_spans() {
    let source = "type Query { user(id: ID!): User }\nfragment Root on Query { user { id } }\nquery Get { ...Root }\n";
    let facts = extract(source);
    assert!(facts.diagnostics.is_empty());
    assert!(
        !facts.contracts.is_empty(),
        "advertised valid GraphQL cannot fall back to empty facts"
    );
    assert!(facts.contracts.iter().any(|fact| {
        fact.name == "user"
            && matches!(
                fact.kind,
                ContractKind::GraphqlField {
                    operation: Some(GraphqlOperation::Query),
                    ref return_type,
                } if return_type == "User"
            )
            && &source[fact.span.start..fact.span.end] == "user"
    }));
    assert!(
        facts.contracts.iter().any(|fact| {
            fact.name == "Root" && fact.kind == ContractKind::GraphqlFragmentSpread
        })
    );
}

#[test]
fn invalid_graphql_fails_closed() {
    for source in [
        "type Query { broken: String",
        "query Missing\ntype Query { okay: String }",
    ] {
        let facts = extract(source);
        assert!(facts.contracts.is_empty());
        assert_eq!(facts.diagnostics[0].code, "graphql.syntax_error");
        let span = facts.diagnostics[0].span;
        assert!(
            span.end > span.start,
            "diagnostic must identify the exact offending token"
        );
        assert!(!source[span.start..span.end].is_empty());
    }
}
