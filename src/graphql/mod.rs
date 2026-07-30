//! Lossless GraphQL SDL and executable-document contract facts.

use crate::contract_tokens::ContractTokens;
use crate::facts::{Contract, ContractKind, Facts, GraphqlOperation, GraphqlType, ParseDiagnostic};
use crate::syntax::Language;
use crate::token::TokenKind;
use std::collections::BTreeMap;

#[must_use]
pub fn extract(source: &str) -> Facts {
    Parser::new(source).parse()
}

struct Parser<'source> {
    tokens: ContractTokens<'source>,
    roots: BTreeMap<String, GraphqlOperation>,
    facts: Facts,
}

mod lifecycle;
mod navigation;
mod operations;
mod schema;

fn operation(value: &str) -> Option<GraphqlOperation> {
    match value {
        "query" => Some(GraphqlOperation::Query),
        "mutation" => Some(GraphqlOperation::Mutation),
        "subscription" => Some(GraphqlOperation::Subscription),
        _ => None,
    }
}

fn operation_name(operation: GraphqlOperation) -> &'static str {
    match operation {
        GraphqlOperation::Query => "query",
        GraphqlOperation::Mutation => "mutation",
        GraphqlOperation::Subscription => "subscription",
    }
}

#[cfg(test)]
mod tests;
