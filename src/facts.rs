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
    /// A CSS class or id selector, named with its leading `.` or `#`.
    Selector,
    /// An infrastructure object: a Terraform resource, data source or output.
    Resource,
    /// A section heading in a document.
    Heading,
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
    /// Local names this import binds.
    ///
    /// Without them a consumer meeting `router` in `app.use("/api", router)`
    /// cannot tell which module it came from, and the mount resolves to
    /// nothing.
    pub names: Vec<String>,
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
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
}

impl Facts {
    /// Just the call sites, for consumers that want a call graph and nothing
    /// else.
    pub fn calls(&self) -> impl Iterator<Item = &Reference> {
        self.references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Call)
    }
}
