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
}

/// A named declaration and where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub name: String,
    pub kind: DeclarationKind,
    pub span: Span,
    /// Enclosing declaration, when the language nests them.
    pub owner: Option<String>,
    /// Whether the declaration leaves the module.
    pub exported: bool,
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
}

/// A call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The called name, without its receiver.
    pub name: String,
    /// Receiver written before the final dot, when there was one.
    pub receiver: Option<String>,
    pub span: Span,
    /// Enclosing declaration the call was written in.
    pub owner: Option<String>,
    /// Literal string arguments, which carry routes, topics and table names.
    pub string_arguments: Vec<String>,
}

/// Everything one structural pass found in one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub declarations: Vec<Declaration>,
    pub imports: Vec<Import>,
    pub calls: Vec<Call>,
}
