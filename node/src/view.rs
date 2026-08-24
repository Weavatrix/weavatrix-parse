//! Borrowed serialization views.
//!
//! The Node surface returns one JSON document per call. Serializing borrowed
//! views straight from `weavatrix-parse` output keeps the boundary linear in
//! the emitted bytes instead of building an intermediate value tree and a
//! second copy of every name, span, and token.

use serde::Serialize;
use weavatrix_parse::{
    Contract, ContractKind, Declaration, DeclarationKind, Facts, GraphqlOperation, GraphqlType,
    Import, ParseDiagnostic, Reference, ReferenceKind, Span, Token, TokenKind,
};

#[derive(Serialize)]
pub(crate) struct FactsView<'a> {
    declarations: Vec<DeclarationView<'a>>,
    imports: Vec<ImportView<'a>>,
    references: Vec<ReferenceView<'a>>,
    contracts: Vec<ContractView<'a>>,
    diagnostics: Vec<DiagnosticView<'a>>,
}

impl<'a> FactsView<'a> {
    pub(crate) fn new(facts: &'a Facts) -> Self {
        Self {
            declarations: facts
                .declarations
                .iter()
                .map(|item| DeclarationView::new(facts, item))
                .collect(),
            imports: facts.imports.iter().map(ImportView::new).collect(),
            references: facts.references.iter().map(ReferenceView::new).collect(),
            contracts: facts.contracts.iter().map(ContractView::new).collect(),
            diagnostics: facts.diagnostics.iter().map(DiagnosticView::new).collect(),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpanView {
    start: usize,
    end: usize,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
}

impl From<Span> for SpanView {
    fn from(value: Span) -> Self {
        Self {
            start: value.start,
            end: value.end,
            line: value.line,
            column: value.column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeclarationView<'a> {
    name: &'a str,
    kind: &'static str,
    span: SpanView,
    extent: SpanView,
    owner: Option<&'a str>,
    exported: bool,
    test_only: bool,
}

impl<'a> DeclarationView<'a> {
    fn new(facts: &Facts, item: &'a Declaration) -> Self {
        Self {
            name: &item.name,
            kind: declaration_kind(item.kind),
            span: item.span.into(),
            extent: item.extent.into(),
            owner: item.owner.as_deref(),
            exported: item.exported,
            test_only: facts.declaration_is_test_only(item.span),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportView<'a> {
    specifier: &'a str,
    span: SpanView,
    type_only: bool,
    reexport: bool,
    names: &'a [String],
    bindings: Vec<BindingView<'a>>,
}

impl<'a> ImportView<'a> {
    fn new(item: &'a Import) -> Self {
        Self {
            specifier: &item.specifier,
            span: item.span.into(),
            type_only: item.type_only,
            reexport: item.reexport,
            names: &item.names,
            bindings: item
                .bindings
                .iter()
                .map(|binding| BindingView {
                    imported: &binding.imported,
                    local: &binding.local,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct BindingView<'a> {
    imported: &'a str,
    local: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceView<'a> {
    name: &'a str,
    kind: &'static str,
    receiver: Option<&'a str>,
    span: SpanView,
    owner: Option<&'a str>,
    string_arguments: &'a [String],
    name_arguments: &'a [String],
}

impl<'a> ReferenceView<'a> {
    fn new(item: &'a Reference) -> Self {
        Self {
            name: &item.name,
            kind: reference_kind(item.kind),
            receiver: item.receiver.as_deref(),
            span: item.span.into(),
            owner: item.owner.as_deref(),
            string_arguments: &item.string_arguments,
            name_arguments: &item.name_arguments,
        }
    }
}

#[derive(Serialize)]
struct ContractView<'a> {
    name: &'a str,
    kind: ContractKindView<'a>,
    span: SpanView,
    owner: Option<&'a str>,
}

impl<'a> ContractView<'a> {
    fn new(item: &'a Contract) -> Self {
        Self {
            name: &item.name,
            kind: ContractKindView::new(&item.kind),
            span: item.span.into(),
            owner: item.owner.as_deref(),
        }
    }
}

#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum ContractKindView<'a> {
    GraphqlType {
        graphql_type: &'static str,
    },
    GraphqlField {
        operation: Option<&'static str>,
        return_type: &'a str,
    },
    GraphqlOperation {
        operation: &'static str,
    },
    GraphqlCall {
        operation: &'static str,
    },
    GraphqlFragment {
        on_type: &'a str,
        operation: Option<&'static str>,
    },
    GraphqlFragmentSpread,
    ProtobufPackage,
    ProtobufMessage,
    ProtobufEnum,
    ProtobufService,
    ProtobufRpc {
        input: &'a str,
        output: &'a str,
        client_streaming: bool,
        server_streaming: bool,
    },
    Unknown,
}

impl<'a> ContractKindView<'a> {
    fn new(kind: &'a ContractKind) -> Self {
        match kind {
            ContractKind::GraphqlType(value) => Self::GraphqlType {
                graphql_type: graphql_type(*value),
            },
            ContractKind::GraphqlField {
                operation,
                return_type,
            } => Self::GraphqlField {
                operation: operation.map(graphql_operation),
                return_type,
            },
            ContractKind::GraphqlOperation(value) => Self::GraphqlOperation {
                operation: graphql_operation(*value),
            },
            ContractKind::GraphqlCall(value) => Self::GraphqlCall {
                operation: graphql_operation(*value),
            },
            ContractKind::GraphqlFragment { on_type, operation } => Self::GraphqlFragment {
                on_type,
                operation: operation.map(graphql_operation),
            },
            ContractKind::GraphqlFragmentSpread => Self::GraphqlFragmentSpread,
            ContractKind::ProtobufPackage => Self::ProtobufPackage,
            ContractKind::ProtobufMessage => Self::ProtobufMessage,
            ContractKind::ProtobufEnum => Self::ProtobufEnum,
            ContractKind::ProtobufService => Self::ProtobufService,
            ContractKind::ProtobufRpc {
                input,
                output,
                client_streaming,
                server_streaming,
            } => Self::ProtobufRpc {
                input,
                output,
                client_streaming: *client_streaming,
                server_streaming: *server_streaming,
            },
            _ => Self::Unknown,
        }
    }
}

#[derive(Serialize)]
struct DiagnosticView<'a> {
    code: &'a str,
    message: &'a str,
    span: SpanView,
}

impl<'a> DiagnosticView<'a> {
    fn new(item: &'a ParseDiagnostic) -> Self {
        Self {
            code: item.code,
            message: &item.message,
            span: item.span.into(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct TokenView<'a> {
    kind: &'static str,
    start: usize,
    end: usize,
    line: u32,
    column: u32,
    text: &'a str,
}

impl<'a> TokenView<'a> {
    pub(crate) fn new(token: &Token, source: &'a str) -> Self {
        Self {
            kind: token_kind(token.kind),
            start: token.start,
            end: token.end,
            line: token.line,
            column: token.column,
            text: token.text(source),
        }
    }
}

fn declaration_kind(value: DeclarationKind) -> &'static str {
    match value {
        DeclarationKind::Function => "function",
        DeclarationKind::Method => "method",
        DeclarationKind::Class => "class",
        DeclarationKind::Interface => "interface",
        DeclarationKind::Enum => "enum",
        DeclarationKind::TypeAlias => "type-alias",
        DeclarationKind::Field => "field",
        DeclarationKind::Constant => "constant",
        DeclarationKind::Variable => "variable",
        DeclarationKind::Module => "module",
        DeclarationKind::Struct => "struct",
        DeclarationKind::Trait => "trait",
        DeclarationKind::Table => "table",
        DeclarationKind::View => "view",
        DeclarationKind::Procedure => "procedure",
        DeclarationKind::Selector => "selector",
        DeclarationKind::Resource => "resource",
        DeclarationKind::Heading => "heading",
        _ => "unknown",
    }
}

fn reference_kind(value: ReferenceKind) -> &'static str {
    match value {
        ReferenceKind::Call => "call",
        ReferenceKind::Inherits => "inherits",
        ReferenceKind::Implements => "implements",
        ReferenceKind::Uses => "uses",
        ReferenceKind::Reads => "reads",
        ReferenceKind::Writes => "writes",
        _ => "unknown",
    }
}

fn graphql_operation(value: GraphqlOperation) -> &'static str {
    match value {
        GraphqlOperation::Query => "query",
        GraphqlOperation::Mutation => "mutation",
        GraphqlOperation::Subscription => "subscription",
    }
}

fn graphql_type(value: GraphqlType) -> &'static str {
    match value {
        GraphqlType::Object => "object",
        GraphqlType::Interface => "interface",
        GraphqlType::Input => "input",
        GraphqlType::Enum => "enum",
        GraphqlType::Scalar => "scalar",
        GraphqlType::Union => "union",
    }
}

fn token_kind(value: TokenKind) -> &'static str {
    match value {
        TokenKind::Whitespace => "whitespace",
        TokenKind::Newline => "newline",
        TokenKind::Indent => "indent",
        TokenKind::LineComment => "line-comment",
        TokenKind::BlockComment => "block-comment",
        TokenKind::String => "string",
        TokenKind::Interpolation => "interpolation",
        TokenKind::Number => "number",
        TokenKind::Identifier => "identifier",
        TokenKind::Regex => "regex",
        TokenKind::Punctuation => "punctuation",
        TokenKind::Unterminated => "unterminated",
        _ => "unknown",
    }
}
