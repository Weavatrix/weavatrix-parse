//! Lossless GraphQL SDL and executable-document contract facts.

use crate::contract_tokens::ContractTokens;
use crate::{
    Contract, ContractKind, Facts, GraphqlOperation, GraphqlType, Language, ParseDiagnostic,
    TokenKind,
};
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

impl<'source> Parser<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            tokens: ContractTokens::new(source, Language::Graphql),
            roots: BTreeMap::new(),
            facts: Facts::default(),
        }
    }

    fn parse(mut self) -> Facts {
        if let Some(error) = self.tokens.delimiter_error("graphql.syntax_error") {
            self.facts.diagnostics.push(error);
            return self.facts;
        }
        self.discover_roots();
        let mut index = 0;
        while index < self.tokens.len() {
            let extended = self.text(index) == "extend";
            let keyword = index + usize::from(extended);
            let value = self.text(keyword).to_owned();
            let required = extended
                || matches!(
                    value.as_str(),
                    "schema"
                        | "type"
                        | "interface"
                        | "input"
                        | "enum"
                        | "scalar"
                        | "union"
                        | "query"
                        | "mutation"
                        | "subscription"
                        | "fragment"
                        | "{"
                );
            let next = match value.as_str() {
                "schema" => self.skip_body(keyword),
                "type" | "interface" | "input" => self.parse_object(keyword, &value),
                "enum" | "scalar" | "union" => self.parse_named_type(keyword, &value),
                "query" => self.parse_operation(keyword, GraphqlOperation::Query),
                "mutation" => self.parse_operation(keyword, GraphqlOperation::Mutation),
                "subscription" => self.parse_operation(keyword, GraphqlOperation::Subscription),
                "fragment" => self.parse_fragment(keyword),
                "{" if !extended => self.parse_anonymous(keyword),
                _ => None,
            };
            if required && next.is_none() {
                return self.fail(keyword, "incomplete or unsupported GraphQL declaration");
            }
            index = next.filter(|next| *next > index).unwrap_or(index + 1);
        }
        self.facts.contracts.sort_by_key(|fact| fact.span.start);
        self.facts
    }

    fn text(&self, index: usize) -> &str {
        self.tokens.text(index)
    }

    fn discover_roots(&mut self) {
        for (name, operation) in [
            ("Query", GraphqlOperation::Query),
            ("Mutation", GraphqlOperation::Mutation),
            ("Subscription", GraphqlOperation::Subscription),
        ] {
            self.roots.insert(name.to_owned(), operation);
        }
        let mut index = 0;
        while index < self.tokens.len() {
            if self.text(index) != "schema" {
                index += 1;
                continue;
            }
            let Some(open) = self.body_open(index + 1) else {
                return;
            };
            let Some(close) = self.tokens.matching(open, "{", "}") else {
                return;
            };
            let mut item = open + 1;
            while item < close {
                if let Some(root) = operation(self.text(item))
                    && self.text(item + 1) == ":"
                    && self.identifier(item + 2)
                {
                    self.roots.insert(self.text(item + 2).to_owned(), root);
                    item += 3;
                } else {
                    item += 1;
                }
            }
            index = close + 1;
        }
    }

    fn parse_object(&mut self, keyword: usize, declaration: &str) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) {
            return None;
        }
        let open = self.body_open(name + 1)?;
        let close = self.tokens.matching(open, "{", "}")?;
        let type_kind = match declaration {
            "type" => GraphqlType::Object,
            "interface" => GraphqlType::Interface,
            "input" => GraphqlType::Input,
            _ => return None,
        };
        let owner = self.text(name).to_owned();
        self.facts.contracts.push(Contract {
            name: owner.clone(),
            kind: ContractKind::GraphqlType(type_kind),
            span: self.tokens.span(name),
            owner: None,
        });
        self.parse_fields(open, close, &owner)?;
        Some(close + 1)
    }

    fn parse_named_type(&mut self, keyword: usize, declaration: &str) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) {
            return None;
        }
        let type_kind = match declaration {
            "enum" => GraphqlType::Enum,
            "scalar" => GraphqlType::Scalar,
            "union" => GraphqlType::Union,
            _ => return None,
        };
        let next = if declaration == "enum" {
            let open = self.body_open(name + 1)?;
            self.tokens.matching(open, "{", "}")? + 1
        } else {
            name + 1
        };
        self.facts.contracts.push(Contract {
            name: self.text(name).to_owned(),
            kind: ContractKind::GraphqlType(type_kind),
            span: self.tokens.span(name),
            owner: None,
        });
        Some(next)
    }

    fn parse_fields(&mut self, open: usize, close: usize, owner: &str) -> Option<()> {
        let mut parentheses = 0_u32;
        let mut brackets = 0_u32;
        let mut index = open + 1;
        while index < close {
            match self.text(index) {
                "(" => parentheses = parentheses.saturating_add(1),
                ")" => parentheses = parentheses.saturating_sub(1),
                "[" => brackets = brackets.saturating_add(1),
                "]" => brackets = brackets.saturating_sub(1),
                _ => {}
            }
            if parentheses == 0
                && brackets == 0
                && self.identifier(index)
                && self.text(index.saturating_sub(1)) != "@"
                && matches!(self.text(index + 1), ":" | "(")
            {
                let colon = if self.text(index + 1) == "(" {
                    let argument_close = self.tokens.matching(index + 1, "(", ")")?;
                    if self.text(argument_close + 1) != ":" {
                        return None;
                    }
                    argument_close + 1
                } else {
                    index + 1
                };
                let return_type = self.graphql_type(colon + 1);
                self.facts.contracts.push(Contract {
                    name: self.text(index).to_owned(),
                    kind: ContractKind::GraphqlField {
                        operation: self.roots.get(owner).copied(),
                        return_type,
                    },
                    span: self.tokens.span(index),
                    owner: Some(owner.to_owned()),
                });
            }
            index += 1;
        }
        Some(())
    }

    fn graphql_type(&self, start: usize) -> String {
        let mut output = String::new();
        for index in start..self.tokens.len() {
            let value = self.text(index);
            if self.identifier(index) || matches!(value, "[" | "]" | "!") {
                output.push_str(value);
            } else {
                break;
            }
        }
        output
    }

    fn parse_operation(&mut self, keyword: usize, operation: GraphqlOperation) -> Option<usize> {
        let open = self.body_open(keyword + 1)?;
        let close = self.tokens.matching(open, "{", "}")?;
        let name = (keyword + 1..open).find(|item| self.identifier(*item));
        let owner = name.map_or_else(
            || {
                format!(
                    "<anonymous {}@{}>",
                    operation_name(operation),
                    self.tokens[keyword].line
                )
            },
            |item| self.text(item).to_owned(),
        );
        self.facts.contracts.push(Contract {
            name: owner.clone(),
            kind: ContractKind::GraphqlOperation(operation),
            span: self.tokens.span(name.unwrap_or(keyword)),
            owner: None,
        });
        self.parse_selections(open, close, operation, &owner);
        Some(close + 1)
    }

    fn parse_anonymous(&mut self, open: usize) -> Option<usize> {
        let close = self.tokens.matching(open, "{", "}")?;
        let owner = format!("<anonymous query@{}>", self.tokens[open].line);
        self.facts.contracts.push(Contract {
            name: owner.clone(),
            kind: ContractKind::GraphqlOperation(GraphqlOperation::Query),
            span: self.tokens.span(open),
            owner: None,
        });
        self.parse_selections(open, close, GraphqlOperation::Query, &owner);
        Some(close + 1)
    }

    fn parse_fragment(&mut self, keyword: usize) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) || self.text(name + 1) != "on" || !self.identifier(name + 2) {
            return None;
        }
        let on_type = self.text(name + 2).to_owned();
        let open = self.body_open(name + 3)?;
        let close = self.tokens.matching(open, "{", "}")?;
        let fragment = self.text(name).to_owned();
        let root = self.roots.get(&on_type).copied();
        self.facts.contracts.push(Contract {
            name: fragment.clone(),
            kind: ContractKind::GraphqlFragment {
                on_type,
                operation: root,
            },
            span: self.tokens.span(name),
            owner: None,
        });
        if let Some(operation) = root {
            self.parse_selections(open, close, operation, &fragment);
        }
        Some(close + 1)
    }

    fn parse_selections(
        &mut self,
        open: usize,
        close: usize,
        operation: GraphqlOperation,
        owner: &str,
    ) {
        let mut braces = 0_u32;
        let mut parentheses = 0_u32;
        let mut index = open + 1;
        while index < close {
            match self.text(index) {
                "{" => braces = braces.saturating_add(1),
                "}" => braces = braces.saturating_sub(1),
                "(" => parentheses = parentheses.saturating_add(1),
                ")" => parentheses = parentheses.saturating_sub(1),
                _ => {}
            }
            if braces == 0 && parentheses == 0 {
                if self.text(index) == "."
                    && self.text(index + 1) == "."
                    && self.text(index + 2) == "."
                    && self.identifier(index + 3)
                    && self.text(index + 3) != "on"
                {
                    self.facts.contracts.push(Contract {
                        name: self.text(index + 3).to_owned(),
                        kind: ContractKind::GraphqlFragmentSpread,
                        span: self.tokens.span(index + 3),
                        owner: Some(owner.to_owned()),
                    });
                    index += 4;
                    continue;
                }
                if self.identifier(index)
                    && !matches!(self.text(index.saturating_sub(1)), "@" | "$" | "." | "on")
                    && self.text(index) != "on"
                {
                    let field = if self.text(index + 1) == ":" && self.identifier(index + 2) {
                        index + 2
                    } else {
                        index
                    };
                    self.facts.contracts.push(Contract {
                        name: self.text(field).to_owned(),
                        kind: ContractKind::GraphqlCall(operation),
                        span: self.tokens.span(field),
                        owner: Some(owner.to_owned()),
                    });
                    index = field;
                }
            }
            index += 1;
        }
    }

    fn skip_body(&self, keyword: usize) -> Option<usize> {
        let open = self.body_open(keyword + 1)?;
        self.tokens.matching(open, "{", "}").map(|close| close + 1)
    }

    fn body_open(&self, start: usize) -> Option<usize> {
        let mut parentheses = 0_u32;
        for index in start..self.tokens.len() {
            match self.text(index) {
                "(" => parentheses = parentheses.saturating_add(1),
                ")" => parentheses = parentheses.checked_sub(1)?,
                "{" if parentheses == 0 => return Some(index),
                "schema" | "type" | "interface" | "input" | "enum" | "scalar" | "union"
                | "query" | "mutation" | "subscription" | "fragment"
                    if parentheses == 0
                        && index > start
                        && self.text(index.saturating_sub(1)) != "@" =>
                {
                    return None;
                }
                _ => {}
            }
        }
        None
    }

    fn identifier(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
    }

    fn fail(mut self, index: usize, message: &str) -> Facts {
        self.facts = Facts::default();
        self.facts.diagnostics.push(ParseDiagnostic {
            code: "graphql.syntax_error",
            message: message.to_owned(),
            span: self.tokens.span(index),
        });
        self.facts
    }
}

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
mod tests {
    use super::*;
    use crate::ContractKind;

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
        assert!(facts.contracts.iter().any(|fact| {
            fact.name == "Root" && fact.kind == ContractKind::GraphqlFragmentSpread
        }));
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
}
