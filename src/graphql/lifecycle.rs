use super::{BTreeMap, ContractTokens, Facts, GraphqlOperation, Language, Parser, operation};

impl<'source> Parser<'source> {
    pub(super) fn new(source: &'source str) -> Self {
        Self {
            tokens: ContractTokens::new(source, Language::Graphql),
            roots: BTreeMap::new(),
            facts: Facts::default(),
        }
    }

    pub(super) fn parse(mut self) -> Facts {
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

    pub(super) fn text(&self, index: usize) -> &str {
        self.tokens.text(index)
    }

    pub(super) fn discover_roots(&mut self) {
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
}
