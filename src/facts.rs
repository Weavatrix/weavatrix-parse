//! Language-neutral facts a structural pass extracts from a token stream.
//!
//! These are the shapes repository intelligence actually consumes. Anything a
//! consumer cannot use - operator precedence, expression trees, type
//! inference - is deliberately absent, which is what keeps extraction linear
//! in the token count.

/// Position of a fact in its source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// The GraphQL root operation a field exposes or an executable document calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphqlOperation {
    Query,
    Mutation,
    Subscription,
}

/// The schema role of a GraphQL named type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphqlType {
    Object,
    Interface,
    Input,
    Enum,
    Scalar,
    Union,
}

/// A typed API-contract fact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractKind {
    GraphqlType(GraphqlType),
    GraphqlField {
        operation: Option<GraphqlOperation>,
        return_type: String,
    },
    GraphqlOperation(GraphqlOperation),
    GraphqlCall(GraphqlOperation),
    GraphqlFragment {
        on_type: String,
        operation: Option<GraphqlOperation>,
    },
    GraphqlFragmentSpread,
    ProtobufPackage,
    ProtobufMessage,
    ProtobufEnum,
    ProtobufService,
    ProtobufRpc {
        input: String,
        output: String,
        client_streaming: bool,
        server_streaming: bool,
    },
}

/// A named contract element and its exact source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    pub name: String,
    pub kind: ContractKind,
    pub span: Span,
    pub owner: Option<String>,
}

/// A fail-closed diagnostic emitted instead of guessed structural facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

/// What a declared name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclarationKind {
    Function,
    Method,
    Class,
    Interface,
    Enum,
    TypeAlias,
    Field,
    Constant,
    Variable,
    Module,
    Struct,
    Trait,
    Table,
    View,
    Procedure,
    /// A CSS class or id selector, named with its leading `.` or `#`.
    Selector,
    /// An infrastructure object: a Terraform resource, data source or output.
    Resource,
    /// A section heading in a document.
    Heading,
}

/// A named declaration and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    /// Exact source occupied by the declaration name and its modifiers.
    pub span: Span,
    /// Full source occupied by the declaration, including its body when one exists.
    pub extent: Span,
    /// Enclosing declaration, when the language nests them.
    pub owner: Option<String>,
    /// Whether the declaration leaves the module.
    pub exported: bool,
}

/// A module this file pulls in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBinding {
    /// The name exported by the imported module.
    pub imported: String,
    /// The name made available in this file.
    pub local: String,
}

/// A module this file pulls in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    /// The specifier exactly as written, without quotes.
    pub specifier: String,
    pub span: Span,
    /// A type-position import, which disappears when the code is compiled.
    pub type_only: bool,
    /// `export ... from`, which forwards another module's surface.
    pub reexport: bool,
    /// Local names this import binds.
    ///
    /// Without them a consumer meeting `router` in `app.use("/api", router)`
    /// cannot tell which module it came from, and the mount resolves to
    /// nothing.
    pub names: Vec<String>,
    /// Lossless exported-to-local binding pairs.
    ///
    /// `names` remains the backward-compatible list of local names. This field
    /// preserves the source name too, so `import { original as local }` can be
    /// resolved to `original` without a repository-wide guess for `local`.
    pub bindings: Vec<ImportBinding>,
}

/// Why one name mentions another.
///
/// A call and an `extends` clause are both "this name depends on that name",
/// and separating them into different fact types would force every consumer to
/// walk two collections to answer one question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReferenceKind {
    Call,
    Inherits,
    Implements,
    /// A name used without being called, as an HTML `class` attribute uses a
    /// CSS selector.
    Uses,
    /// A statement that reads the named object, as `SELECT ... FROM users`.
    Reads,
    /// A statement that writes it, as `INSERT INTO users`.
    Writes,
}

/// One name mentioning another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// The referenced name, without its receiver.
    pub name: String,
    pub kind: ReferenceKind,
    /// Receiver written before the final dot, when there was one.
    pub receiver: Option<String>,
    pub span: Span,
    /// Enclosing declaration the reference was written in.
    pub owner: Option<String>,
    /// Literal string arguments, which carry routes, topics and table names.
    pub string_arguments: Vec<String>,
    /// Names passed as arguments, in the order written.
    ///
    /// `app.use("/api", router)` mounts one module under a prefix, and the
    /// prefix is a string while the module is a name - so a consumer that
    /// only sees literals sees half the fact and can resolve neither end.
    pub name_arguments: Vec<String>,
}

/// Everything one structural pass found in one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub declarations: Vec<Declaration>,
    /// Exact spans of declarations that exist only in a test compilation.
    ///
    /// Keeping this sparse avoids enlarging every declaration in every
    /// language. Rust fills it from `#[test]` and positive `#[cfg(test)]`
    /// contexts; languages without compile-time test scopes leave it empty.
    pub test_only_declarations: Vec<Span>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub contracts: Vec<Contract>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl Facts {
    /// Whether the declaration at `span` only exists in a test compilation.
    #[must_use]
    pub fn declaration_is_test_only(&self, span: Span) -> bool {
        self.test_only_declarations.contains(&span)
    }

    /// Just the call sites, for consumers that want a call graph and nothing
    /// else.
    pub fn calls(&self) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Call)
    }
}
