use super::{Contract, ContractKind, GraphqlOperation, Parser, operation_name};

impl Parser<'_> {
    pub(super) fn parse_operation(
        &mut self,
        keyword: usize,
        operation: GraphqlOperation,
    ) -> Option<usize> {
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

    pub(super) fn parse_anonymous(&mut self, open: usize) -> Option<usize> {
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

    pub(super) fn parse_fragment(&mut self, keyword: usize) -> Option<usize> {
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

    pub(super) fn parse_selections(
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
}
