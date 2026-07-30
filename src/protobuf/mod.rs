//! Lossless protobuf package, message, service, and RPC contract facts.

use crate::contract_tokens::ContractTokens;
use crate::facts::{Contract, ContractKind, Facts, Import, ParseDiagnostic, Span};
use crate::syntax::Language;
use crate::token::TokenKind;

#[must_use]
pub fn extract(source: &str) -> Facts {
    Parser::new(source).parse()
}

struct Parser<'source> {
    tokens: ContractTokens<'source>,
    package: String,
    message_scopes: Vec<(usize, String)>,
    facts: Facts,
}

mod parser;

#[cfg(test)]
mod tests;
