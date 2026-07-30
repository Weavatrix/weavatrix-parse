use super::{
    Contract, ContractKind, ContractTokens, Facts, Import, Language, ParseDiagnostic, Parser, Span,
    TokenKind,
};

impl<'source> Parser<'source> {
    pub(super) fn new(source: &'source str) -> Self {
        Self {
            tokens: ContractTokens::new(source, Language::Protobuf),
            package: String::new(),
            message_scopes: Vec::new(),
            facts: Facts::default(),
        }
    }

    pub(super) fn parse(mut self) -> Facts {
        if let Some(error) = self.tokens.delimiter_error("protobuf.syntax_error") {
            self.facts.diagnostics.push(error);
            return self.facts;
        }
        if self.dialect().is_none() {
            return self.fail(
                0,
                "protobuf.invalid_dialect",
                "expected syntax = \"proto2\", syntax = \"proto3\", edition = \"2023\", or edition = \"2024\"",
            );
        }
        let mut index = 0;
        while index < self.tokens.len() {
            self.message_scopes.retain(|(close, _)| index <= *close);
            let required = matches!(
                self.text(index),
                "package" | "import" | "message" | "enum" | "service"
            );
            let next = match self.text(index) {
                "package" => self.parse_package(index),
                "import" => self.parse_import(index),
                "message" => self.parse_type(index, ContractKind::ProtobufMessage),
                "enum" => self.parse_type(index, ContractKind::ProtobufEnum),
                "service" => self.parse_service(index),
                _ => None,
            };
            if required && next.is_none() {
                return self.fail(
                    index,
                    "protobuf.syntax_error",
                    "incomplete protobuf declaration",
                );
            }
            index = next.filter(|next| *next > index).unwrap_or(index + 1);
        }
        self.facts.contracts.sort_by_key(|fact| fact.span.start);
        self.facts
    }

    fn text(&self, index: usize) -> &str {
        self.tokens.text(index)
    }

    fn dialect(&self) -> Option<&str> {
        let declaration = self.text(0);
        let value = self
            .tokens
            .get(2)
            .filter(|token| token.kind == TokenKind::String)
            .map(|_| self.text(2).trim_matches(['"', '\'']))?;
        let recognized = self.text(1) == "="
            && self.text(3) == ";"
            && matches!(
                (declaration, value),
                ("syntax", "proto2" | "proto3") | ("edition", "2023" | "2024")
            );
        let duplicate = (4..self.tokens.len()).any(|index| {
            matches!(self.text(index), "syntax" | "edition")
                && self.text(index + 1) == "="
                && self
                    .tokens
                    .get(index + 2)
                    .is_some_and(|token| token.kind == TokenKind::String)
                && self.text(index + 3) == ";"
        });
        (recognized && !duplicate).then_some(value)
    }

    fn parse_package(&mut self, keyword: usize) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) {
            return None;
        }
        let mut end = name;
        while self.identifier(end) || self.text(end) == "." {
            end += 1;
        }
        if self.text(end) != ";" {
            return None;
        }
        let qualified = self.qualified_name(name, end);
        if qualified.is_empty() {
            return None;
        }
        self.package.clone_from(&qualified);
        self.facts.contracts.push(Contract {
            name: qualified,
            kind: ContractKind::ProtobufPackage,
            span: self.tokens.span(name),
            owner: None,
        });
        Some(end + 1)
    }

    fn parse_import(&mut self, keyword: usize) -> Option<usize> {
        let path = keyword
            + if matches!(self.text(keyword + 1), "public" | "weak" | "option") {
                2
            } else {
                1
            };
        if !self
            .tokens
            .get(path)
            .is_some_and(|token| token.kind == TokenKind::String)
            || self.text(path + 1) != ";"
        {
            return None;
        }
        self.facts.imports.push(Import {
            specifier: self
                .text(path)
                .trim_matches(|character| matches!(character, '"' | '\''))
                .to_owned(),
            span: self.tokens.span(path),
            type_only: false,
            reexport: self.text(keyword + 1) == "public",
            names: Vec::new(),
            bindings: Vec::new(),
        });
        Some(path + 2)
    }

    fn parse_type(&mut self, keyword: usize, kind: ContractKind) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) || self.text(name + 1) != "{" {
            return None;
        }
        let close = self.tokens.matching(name + 1, "{", "}")?;
        let simple_name = self.text(name);
        let owner = self.message_scopes.last().map(|(_, owner)| owner.clone());
        let qualified_name = owner.as_ref().map_or_else(
            || simple_name.to_owned(),
            |owner| format!("{owner}.{simple_name}"),
        );
        let message = kind == ContractKind::ProtobufMessage;
        self.facts.contracts.push(Contract {
            name: qualified_name.clone(),
            kind,
            span: self.tokens.span(name),
            owner,
        });
        if message {
            self.message_scopes.push((close, qualified_name));
        }
        // Nested messages and enums remain visible to the main pass.
        Some(name + 1)
    }

    fn parse_service(&mut self, keyword: usize) -> Option<usize> {
        let name = keyword + 1;
        if !self.identifier(name) || self.text(name + 1) != "{" {
            return None;
        }
        let open = name + 1;
        let close = self.tokens.matching(open, "{", "}")?;
        let service = self.text(name).to_owned();
        let mut contracts = vec![Contract {
            name: service.clone(),
            kind: ContractKind::ProtobufService,
            span: self.tokens.span(name),
            owner: None,
        }];
        let mut braces = 0_u32;
        let mut index = open + 1;
        while index < close {
            match self.text(index) {
                "{" => braces = braces.saturating_add(1),
                "}" => braces = braces.saturating_sub(1),
                "rpc" if braces == 0 => {
                    let (next, rpc) = self.parse_rpc(index, &service)?;
                    contracts.push(rpc);
                    index = next;
                    continue;
                }
                _ => {}
            }
            index += 1;
        }
        self.facts.contracts.extend(contracts);
        Some(close + 1)
    }

    fn parse_rpc(&self, keyword: usize, service: &str) -> Option<(usize, Contract)> {
        let name = keyword + 1;
        if !self.identifier(name) {
            return None;
        }
        let input_open = name + 1;
        if self.text(input_open) != "(" {
            return None;
        }
        let input_close = self.tokens.matching(input_open, "(", ")")?;
        let returns = input_close + 1;
        if self.text(returns) != "returns" {
            return None;
        }
        let output_open = returns + 1;
        if self.text(output_open) != "(" {
            return None;
        }
        let output_close = self.tokens.matching(output_open, "(", ")")?;
        let (input, client_streaming) = self.rpc_type(input_open + 1, input_close)?;
        let (output, server_streaming) = self.rpc_type(output_open + 1, output_close)?;
        let end = match self.text(output_close + 1) {
            "{" => self.tokens.matching(output_close + 1, "{", "}")? + 1,
            ";" => output_close + 2,
            _ => return None,
        };
        Some((
            end,
            Contract {
                name: self.text(name).to_owned(),
                kind: ContractKind::ProtobufRpc {
                    input,
                    output,
                    client_streaming,
                    server_streaming,
                },
                span: self.tokens.span(name),
                owner: Some(service.to_owned()),
            },
        ))
    }

    fn rpc_type(&self, start: usize, end: usize) -> Option<(String, bool)> {
        let streaming = (start..end).any(|index| self.text(index) == "stream");
        let name =
            (start..end).find(|index| self.identifier(*index) && self.text(*index) != "stream")?;
        let qualified = self.qualified_name(name, end);
        (!qualified.is_empty()).then_some((qualified, streaming))
    }

    fn qualified_name(&self, start: usize, end: usize) -> String {
        let mut name = String::new();
        let mut index = start;
        while index < end {
            if self.identifier(index) || self.text(index) == "." {
                name.push_str(self.text(index));
                index += 1;
            } else {
                break;
            }
        }
        name.trim_matches('.').to_owned()
    }

    fn identifier(&self, index: usize) -> bool {
        self.tokens
            .get(index)
            .is_some_and(|token| token.kind == TokenKind::Identifier)
    }

    fn fail(mut self, index: usize, code: &'static str, message: &str) -> Facts {
        self.facts = Facts::default();
        self.facts.diagnostics.push(ParseDiagnostic {
            code,
            message: message.to_owned(),
            span: if self.tokens.get(index).is_some() {
                self.tokens.span(index)
            } else {
                Span {
                    start: 0,
                    end: 0,
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                }
            },
        });
        self.facts
    }
}
