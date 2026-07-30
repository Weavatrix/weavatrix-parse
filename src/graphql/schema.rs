use super::{Contract, ContractKind, GraphqlType, Parser};

impl Parser<'_> {
    pub(super) fn parse_object(&mut self, keyword: usize, declaration: &str) -> Option<usize> {
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

    pub(super) fn parse_named_type(&mut self, keyword: usize, declaration: &str) -> Option<usize> {
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

    pub(super) fn parse_fields(&mut self, open: usize, close: usize, owner: &str) -> Option<()> {
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

    pub(super) fn graphql_type(&self, start: usize) -> String {
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
}
