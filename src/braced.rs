//! Structural extraction for the brace-scoped languages.
//!
//! Rust, Go, Java, C#, C, C++ and Solidity differ in which keyword introduces
//! a declaration and how a module is named, and agree on everything else:
//! braces open bodies, a name followed by a parameter list is callable, and a
//! call is an identifier followed by `(`. Those differences are tables, so one
//! walk serves all seven instead of seven near-identical scanners - and adding
//! the next such language costs a table, not a scanner.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one brace-scoped source file.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    let mut state = Extractor {
        source,
        tokens: &tokens,
        rules: Rules::of(language),
        facts: Facts::default(),
        scopes: Vec::new(),
        depth: 0,
    };
    state.run();
    state.facts
}

/// Keywords one language uses, as data.
struct Rules {
    /// Keyword to the kind it declares.
    declarations: &'static [(&'static str, DeclarationKind)],
    /// Keywords that introduce a module dependency.
    imports: &'static [&'static str],
    /// Modifiers to step over before the declaring keyword.
    modifiers: &'static [&'static str],
    /// Whether a bare `name(` at type-body depth declares a method.
    braced_members: bool,
    /// Whether a function is declared by a return type rather than a keyword,
    /// as C and C++ do: `int add(int a, int b) { }`.
    typed_functions: bool,
    /// Whether a declaration is public by keyword rather than by convention.
    exported_keyword: Option<&'static str>,
    /// Keywords that open a named scope without declaring anything: Rust's
    /// `impl Type` and Swift's `extension Type` say what the members belong
    /// to, and declare no new name.
    scope_keywords: &'static [&'static str],
}

impl Rules {
    // One arm per language, each a table of keywords. Splitting it to satisfy
    // a line count would scatter the tables and make the languages harder to
    // compare against each other, which is the point of writing them as data.
    #[allow(clippy::too_many_lines)]
    const fn of(language: Language) -> Self {
        match language {
            Language::Rust => Self {
                declarations: &[
                    ("fn", DeclarationKind::Function),
                    ("struct", DeclarationKind::Struct),
                    ("enum", DeclarationKind::Enum),
                    ("trait", DeclarationKind::Trait),
                    ("type", DeclarationKind::TypeAlias),
                    ("const", DeclarationKind::Constant),
                    ("static", DeclarationKind::Constant),
                    ("mod", DeclarationKind::Module),
                ],
                imports: &["use", "mod"],
                modifiers: &["pub", "async", "unsafe", "extern", "default"],
                braced_members: false,
                typed_functions: false,
                exported_keyword: Some("pub"),
                scope_keywords: &["impl"],
            },
            Language::Swift => Self {
                declarations: &[
                    ("func", DeclarationKind::Function),
                    ("class", DeclarationKind::Class),
                    ("struct", DeclarationKind::Struct),
                    ("actor", DeclarationKind::Class),
                    ("enum", DeclarationKind::Enum),
                    ("protocol", DeclarationKind::Interface),
                    ("typealias", DeclarationKind::TypeAlias),
                    ("associatedtype", DeclarationKind::TypeAlias),
                    ("let", DeclarationKind::Constant),
                    ("var", DeclarationKind::Variable),
                    ("init", DeclarationKind::Method),
                    ("subscript", DeclarationKind::Method),
                ],
                imports: &["import"],
                modifiers: &[
                    "public",
                    "private",
                    "internal",
                    "fileprivate",
                    "open",
                    "static",
                    "final",
                    "override",
                    "mutating",
                    "nonmutating",
                    "lazy",
                    "weak",
                    "unowned",
                    "required",
                    "convenience",
                    "indirect",
                    "dynamic",
                    "optional",
                    "async",
                    "throws",
                ],
                braced_members: false,
                typed_functions: false,
                // `open` is wider than `public`, but both leave the module,
                // and `exported` records only whether it leaves.
                exported_keyword: Some("public"),
                scope_keywords: &["extension"],
            },
            Language::Go => Self {
                declarations: &[
                    ("func", DeclarationKind::Function),
                    ("type", DeclarationKind::Struct),
                    ("const", DeclarationKind::Constant),
                    ("var", DeclarationKind::Variable),
                ],
                imports: &["import"],
                modifiers: &[],
                braced_members: false,
                typed_functions: false,
                exported_keyword: None,
                scope_keywords: &[],
            },
            Language::Java | Language::CSharp => Self {
                declarations: &[
                    ("class", DeclarationKind::Class),
                    ("interface", DeclarationKind::Interface),
                    ("enum", DeclarationKind::Enum),
                    ("record", DeclarationKind::Struct),
                    ("struct", DeclarationKind::Struct),
                ],
                imports: &["import", "using"],
                modifiers: &[
                    "public",
                    "private",
                    "protected",
                    "static",
                    "final",
                    "abstract",
                    "sealed",
                    "internal",
                    "override",
                    "async",
                    "virtual",
                    "readonly",
                ],
                braced_members: true,
                typed_functions: false,
                exported_keyword: Some("public"),
                scope_keywords: &[],
            },
            Language::Solidity => Self {
                declarations: &[
                    ("contract", DeclarationKind::Class),
                    ("library", DeclarationKind::Class),
                    ("interface", DeclarationKind::Interface),
                    ("struct", DeclarationKind::Struct),
                    ("enum", DeclarationKind::Enum),
                    ("function", DeclarationKind::Function),
                    ("constructor", DeclarationKind::Method),
                    ("modifier", DeclarationKind::Function),
                    ("event", DeclarationKind::Field),
                    ("error", DeclarationKind::Struct),
                ],
                imports: &["import"],
                modifiers: &[
                    "abstract", "virtual", "override", "public", "private", "internal", "external",
                    "pure", "view", "payable",
                ],
                braced_members: false,
                typed_functions: false,
                // Anything not marked internal or private is reachable from
                // another contract, which is what export means here.
                exported_keyword: Some("public"),
                scope_keywords: &[],
            },
            _ => Self {
                declarations: &[
                    ("struct", DeclarationKind::Struct),
                    ("class", DeclarationKind::Class),
                    ("enum", DeclarationKind::Enum),
                    ("namespace", DeclarationKind::Module),
                ],
                imports: &["#include", "include"],
                modifiers: &["static", "inline", "extern", "const", "virtual"],
                braced_members: true,
                typed_functions: true,
                exported_keyword: None,
                scope_keywords: &[],
            },
        }
    }
}

struct Scope {
    name: String,
    depth: Option<i32>,
    type_body: bool,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    rules: Rules,
    facts: Facts,
    scopes: Vec<Scope>,
    depth: i32,
}

impl Extractor<'_, '_> {
    fn run(&mut self) {
        let mut index = 0;
        while index < self.tokens.len() {
            self.close_scopes();
            index = self.step(index);
        }
    }

    fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let last_index = self.tokens.len().saturating_sub(1);
        let first = &self.tokens[start.min(last_index)];
        let last = &self.tokens[end.min(last_index)];
        Span {
            start: first.start,
            end: last.end,
            line: first.line,
            column: first.column,
            end_line: last.line,
            end_column: last.column,
        }
    }

    fn owner(&self) -> Option<String> {
        self.scopes.last().map(|scope| scope.name.clone())
    }

    fn close_scopes(&mut self) {
        while self
            .scopes
            .last()
            .is_some_and(|scope| scope.depth.is_some_and(|depth| self.depth < depth))
        {
            self.scopes.pop();
        }
    }

    /// Discards a scope still waiting for a body when a second declaration
    /// arrives, because only one can be waiting at a time.
    ///
    /// A semicolon already does this for languages that write one. Swift and
    /// Go end a statement at the newline, so without this a `let name: String`
    /// stayed open and adopted every function declared after it.
    fn drop_waiting(&mut self) {
        if self
            .scopes
            .last()
            .is_some_and(|scope| scope.depth.is_none())
        {
            self.scopes.pop();
        }
    }

    fn open_body(&mut self) {
        let depth = self.depth;
        if let Some(scope) = self.scopes.last_mut()
            && scope.depth.is_none()
        {
            scope.depth = Some(depth);
        }
    }

    fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            self.depth += 1;
            self.open_body();
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            return index + 1;
        }
        if self.punct(index, ";") {
            // A declaration whose statement ended before any brace has no
            // body, so it must not stay open and adopt what follows it -
            // which is what a Solidity `event` did to the next function.
            if self
                .scopes
                .last()
                .is_some_and(|scope| scope.depth.is_none())
            {
                self.scopes.pop();
            }
            return index + 1;
        }
        if self.kind(index) != Some(TokenKind::Identifier) {
            return index + 1;
        }
        if let Some(next) = self.import(index) {
            return next;
        }
        if let Some(next) = self.declaration(index) {
            return next;
        }
        if let Some(next) = self.call(index) {
            return next;
        }
        index + 1
    }

    /// The module forms these languages write: `use a::b;`, `mod x;`,
    /// `import "path"`, grouped Go imports, `import a.b.C;`, `using X;`.
    fn import(&mut self, start: usize) -> Option<usize> {
        // `pub mod x;` is still a module dependency, so modifiers are stepped
        // over here exactly as a declaration would step over them.
        let mut index = start;
        while self.rules.modifiers.contains(&self.text(index)) {
            index += 1;
            // `pub(crate) use x;` carries a parenthesised visibility scope.
            if self.punct(index, "(") {
                while index < self.tokens.len() && !self.punct(index, ")") {
                    index += 1;
                }
                index += 1;
            }
        }
        let word = self.text(index);
        if !self.rules.imports.contains(&word) {
            return None;
        }
        // A parenthesised block lists several paths, as Go writes them.
        if self.punct(index + 1, "(") {
            let mut cursor = index + 2;
            let limit = (index + 512).min(self.tokens.len());
            while cursor < limit && !self.punct(cursor, ")") {
                if self.kind(cursor) == Some(TokenKind::String) {
                    self.facts.imports.push(Import {
                        specifier: self.text(cursor).trim_matches(['"', '`']).to_owned(),
                        span: self.span(cursor, cursor),
                        type_only: false,
                        reexport: false,
                    });
                }
                cursor += 1;
            }
            return Some(cursor + 1);
        }
        // A `mod x { ... }` with a body defines the module here; only a
        // declaration without a body pulls in another file.
        if word == "mod" {
            let name = self.text(index + 1);
            if name.is_empty() || !self.punct(index + 2, ";") {
                return None;
            }
            self.facts.imports.push(Import {
                specifier: format!("self::{name}"),
                span: self.span(index, index + 1),
                type_only: false,
                reexport: false,
            });
            return Some(index + 2);
        }
        // A quoted path is the whole specifier; otherwise the specifier runs
        // to the statement end.
        let line = self.tokens[index].line;
        let mut cursor = index + 1;
        let mut specifier = String::new();
        let limit = (index + 128).min(self.tokens.len());
        while cursor < limit {
            if self.kind(cursor) == Some(TokenKind::String) {
                // A quoted path is the whole specifier, so anything read
                // before it was a list of imported names, not a path.
                specifier.clear();
                specifier.push_str(self.text(cursor).trim_matches(['"', '`', '\'']));
                cursor += 1;
                break;
            }
            if self.punct(cursor, ";") {
                break;
            }
            if self.punct(cursor, "{") {
                // A trailing group narrows a path already read, as in
                // `use a::{b, c}`. A leading one lists names being imported
                // from a path still to come, as in `import {A} from "./x"`.
                if !specifier.is_empty() {
                    break;
                }
                while cursor < limit && !self.punct(cursor, "}") {
                    cursor += 1;
                }
                cursor += 1;
                continue;
            }
            // A specifier ends with its line. Reading on would swallow what
            // follows, which is how `#include <stdio.h>` consumed the function
            // defined beneath it. While nothing has been read yet the scan may
            // still cross a line, because `import {A} from` puts its path on
            // the next one.
            if self.tokens[cursor].line != line && !specifier.is_empty() {
                break;
            }
            if matches!(
                self.kind(cursor),
                Some(TokenKind::Identifier | TokenKind::Punctuation)
            ) {
                specifier.push_str(self.text(cursor));
            }
            cursor += 1;
        }
        // `use a::b::{c, d}` stops at the brace, leaving a separator behind.
        let specifier = specifier.trim_end_matches([':', '.']).to_owned();
        if specifier.is_empty() {
            return None;
        }
        self.facts.imports.push(Import {
            specifier,
            span: self.span(index, cursor.saturating_sub(1)),
            type_only: false,
            reexport: false,
        });
        Some(cursor)
    }

    fn declaration(&mut self, index: usize) -> Option<usize> {
        let mut cursor = index;
        let mut exported = false;
        loop {
            let word = self.text(cursor);
            if self.rules.exported_keyword == Some(word) {
                exported = true;
            }
            if self.rules.modifiers.contains(&word) {
                cursor += 1;
                // `pub(crate)` and similar carry a parenthesised scope.
                if self.punct(cursor, "(") {
                    while cursor < self.tokens.len() && !self.punct(cursor, ")") {
                        cursor += 1;
                    }
                    cursor += 1;
                }
                continue;
            }
            break;
        }
        if self.rules.scope_keywords.contains(&self.text(cursor))
            && let Some(next) = self.open_named_scope(cursor)
        {
            return Some(next);
        }
        let keyword = self.text(cursor);
        let Some((_, kind)) = self
            .rules
            .declarations
            .iter()
            .find(|(word, _)| *word == keyword)
        else {
            return self
                .typed_function(cursor, exported)
                .or_else(|| self.braced_member(cursor, exported));
        };
        let name_index = cursor + 1;
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(name_index).to_owned();
        // Go marks export by an initial capital rather than a keyword.
        let exported = exported || name.starts_with(char::is_uppercase);
        self.drop_waiting();
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: *kind,
            span: self.span(index, name_index),
            owner: self.owner(),
            exported,
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: matches!(
                kind,
                DeclarationKind::Class
                    | DeclarationKind::Struct
                    | DeclarationKind::Interface
                    | DeclarationKind::Trait
                    | DeclarationKind::Enum
            ),
        });
        Some(name_index + 1)
    }

    /// `impl Type`, `impl Trait for Type` and `extension Type` name what their
    /// members belong to without declaring anything themselves.
    ///
    /// The name is the last one before the brace, which is what makes
    /// `impl Display for Engine` belong to `Engine` rather than to `Display`.
    fn open_named_scope(&mut self, keyword: usize) -> Option<usize> {
        self.drop_waiting();
        let limit = (keyword + 64).min(self.tokens.len());
        let mut cursor = keyword + 1;
        let mut name = None;
        let mut generic = 0_i32;
        while cursor < limit && !self.punct(cursor, "{") {
            if self.punct(cursor, ";") {
                return None;
            }
            // A generic argument is part of the type, not a name of its own.
            if self.punct(cursor, "<") {
                generic += 1;
            } else if self.punct(cursor, ">") {
                generic -= 1;
            } else if generic == 0 && self.kind(cursor) == Some(TokenKind::Identifier) {
                name = Some(self.text(cursor).to_owned());
            }
            cursor += 1;
        }
        let name = name?;
        if !self.punct(cursor, "{") {
            return None;
        }
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: true,
        });
        Some(cursor)
    }

    /// A C or C++ function, which no keyword introduces: what marks it is a
    /// return type before the name and a body after the parameter list.
    ///
    /// Without this, `int add(int a, int b) { }` matched nothing and then fell
    /// through to the call path, so every C function definition was recorded
    /// as a call to itself - a self-edge in the graph, and no declaration for
    /// dead-code analysis to find.
    fn typed_function(&mut self, index: usize, exported: bool) -> Option<usize> {
        if !self.rules.typed_functions || !self.punct(index + 1, "(") {
            return None;
        }
        let name = self.text(index);
        // A control structure is also a name followed by a parenthesis and a
        // brace, and `else if (x) {` even has an identifier before it.
        if matches!(
            name,
            "if" | "for" | "while" | "switch" | "return" | "catch" | "sizeof" | "do"
        ) {
            return None;
        }
        // What precedes the name decides: a return type, possibly through a
        // `Class::` qualifier, means a definition; anything else means a call.
        let (owner, type_index) = if self.punct(index - 1, ":")
            && self.punct(index.checked_sub(2)?, ":")
            && self.kind(index.checked_sub(3)?) == Some(TokenKind::Identifier)
        {
            (Some(self.text(index - 3).to_owned()), index.checked_sub(4)?)
        } else {
            (self.owner(), index.checked_sub(1)?)
        };
        let preceded_by_type = self.kind(type_index) == Some(TokenKind::Identifier)
            && !matches!(self.text(type_index), "return" | "else" | "case" | "goto")
            || self.punct(type_index, "*")
            || self.punct(type_index, "&");
        if !preceded_by_type {
            return None;
        }
        // Only a body proves a definition. A prototype ends at a semicolon,
        // and so does `return helper(x);` - so prototypes are left alone
        // rather than risk reading every call as a declaration.
        let mut cursor = index + 2;
        let mut depth = 1_i32;
        let limit = (index + 512).min(self.tokens.len());
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "(") {
                depth += 1;
            } else if self.punct(cursor, ")") {
                depth -= 1;
            }
            cursor += 1;
        }
        // `const`, `noexcept` and `override` may sit between `)` and the body.
        while cursor < limit && self.kind(cursor) == Some(TokenKind::Identifier) {
            cursor += 1;
        }
        if !self.punct(cursor, "{") {
            return None;
        }
        let name = name.to_owned();
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: if owner.is_some() {
                DeclarationKind::Method
            } else {
                DeclarationKind::Function
            },
            span: self.span(index, index),
            owner,
            // C has no visibility keyword; a static function is file-local and
            // everything else is linkable.
            exported: exported || !self.is_static(index),
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: false,
        });
        Some(index + 1)
    }

    /// How many tokens the `<...>` group at `index` occupies, or zero when
    /// this is a comparison rather than a type list.
    ///
    /// A closing angle bracket is what tells the two apart: `a < b` never
    /// reaches one before the statement ends.
    ///
    /// This deliberately allocates nothing. Rust writes `Vec<String>` and
    /// `Option<T>` everywhere, so this runs on almost every identifier in a
    /// file and almost always ends in a type rather than a call - collecting
    /// the names here cost a heap allocation per type argument that was then
    /// thrown away, and a third of the extraction throughput with it.
    fn type_argument_span(&self, index: usize) -> usize {
        if !self.punct(index, "<") {
            return 0;
        }
        let limit = (index + 32).min(self.tokens.len());
        let mut cursor = index + 1;
        let mut depth = 1_i32;
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "<") {
                depth += 1;
            } else if self.punct(cursor, ">") {
                depth -= 1;
            } else if self.punct(cursor, ";") || self.punct(cursor, "{") {
                // A statement ended, so the angle bracket was an operator.
                return 0;
            }
            cursor += 1;
        }
        if depth > 0 { 0 } else { cursor - index }
    }

    /// The type names inside a group already known to be one.
    fn type_argument_names(&self, index: usize, length: usize) -> Vec<String> {
        (index..index + length)
            .filter(|cursor| self.kind(*cursor) == Some(TokenKind::Identifier))
            .map(|cursor| self.text(cursor).to_owned())
            .collect()
    }

    /// Whether the declaration ending at `index` was marked `static`.
    fn is_static(&self, index: usize) -> bool {
        let start = index.saturating_sub(4);
        (start..index).any(|cursor| self.text(cursor) == "static")
    }

    /// A method written directly inside a class or struct body, in languages
    /// that declare members without a keyword.
    fn braced_member(&mut self, index: usize, exported: bool) -> Option<usize> {
        if !self.rules.braced_members {
            return None;
        }
        let inside_type = self.scopes.last().is_some_and(|scope| {
            scope.type_body && scope.depth.is_some_and(|depth| self.depth == depth)
        });
        if !inside_type {
            return None;
        }
        // `Type name(` declares a method; the name is the token before `(`.
        let mut cursor = index;
        let limit = (index + 16).min(self.tokens.len());
        while cursor < limit && !self.punct(cursor + 1, "(") {
            if self.punct(cursor, ";") || self.punct(cursor, "{") || self.punct(cursor, "=") {
                return None;
            }
            cursor += 1;
        }
        if cursor >= limit || self.kind(cursor) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(cursor).to_owned();
        if matches!(name.as_str(), "if" | "for" | "while" | "switch" | "return") {
            return None;
        }
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Method,
            span: self.span(index, cursor),
            owner: self.owner(),
            exported,
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: false,
        });
        Some(cursor + 1)
    }

    fn call(&mut self, index: usize) -> Option<usize> {
        // `modelBuilder.Entity<Order>()` is a call whose parenthesis does not
        // follow the name. The type argument is what an object-relational
        // mapper names the entity with, so it is worth reaching.
        let type_arguments = self.type_argument_span(index + 1);
        let open = index + 1 + type_arguments;
        if !self.punct(open, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(
            name.as_str(),
            "if" | "for" | "while" | "switch" | "match" | "return" | "catch" | "sizeof"
        ) {
            return None;
        }
        let receiver = (index >= 2
            && (self.punct(index - 1, ".") || self.punct(index - 1, ":"))
            && self.kind(index - 2) == Some(TokenKind::Identifier))
        .then(|| self.text(index - 2).to_owned());
        for argument in self.type_argument_names(index + 1, type_arguments) {
            self.facts.references.push(Reference {
                name: argument,
                kind: ReferenceKind::Uses,
                receiver: None,
                span: self.span(index, index),
                owner: self.owner(),
                string_arguments: Vec::new(),
            });
        }
        let mut arguments = Vec::new();
        let mut scan = open + 1;
        let mut depth = 1_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if depth == 1 && self.kind(scan) == Some(TokenKind::String) {
                arguments.push(self.text(scan).trim_matches(['"', '`', '\'']).to_owned());
            }
            scan += 1;
        }
        self.facts.references.push(Reference {
            kind: ReferenceKind::Call,
            name,
            receiver,
            span: self.span(index, index),
            owner: self.owner(),
            string_arguments: arguments,
        });
        Some(index + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::{DeclarationKind, ReferenceKind};
    use crate::syntax::Language;

    fn declared(
        source: &str,
        language: Language,
    ) -> Vec<(String, DeclarationKind, Option<String>)> {
        extract(source, language)
            .declarations
            .into_iter()
            .map(|item| (item.name, item.kind, item.owner))
            .collect()
    }

    #[test]
    fn rust_module_declarations_are_dependencies_but_inline_modules_are_not() {
        let source = "pub mod engine;\nmod helper;\nuse crate::engine::Driver;\n\
             mod inline { pub fn nested() {} }\npub fn run() { Driver::start(); }\n";
        let specifiers = extract(source, Language::Rust)
            .imports
            .into_iter()
            .map(|import| import.specifier)
            .collect::<Vec<_>>();
        assert_eq!(
            specifiers,
            ["self::engine", "self::helper", "crate::engine::Driver"],
            "a mod with a body defines the module here rather than including a file"
        );
    }

    #[test]
    fn a_character_literal_holding_a_quote_does_not_shift_the_rest_of_the_file() {
        // A lifetime and a character literal open the same way, so this used
        // to leave `"` opening a string that ran on for hundreds of lines and
        // swallowed every declaration after it.
        let source = "fn classify<'a>(head: &'a str) -> bool {\n\
             \x20   head.contains(['.', '\"', '+']) || head.starts_with('@')\n\
             }\n\
             mod tests {\n\
             \x20   use super::classify;\n\
             }\n";
        let facts = extract(source, Language::Rust);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["super::classify"],
            "the import after the quote character is still reachable"
        );
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.name == "classify"),
            "the lifetime is punctuation, not an unterminated literal"
        );
    }

    #[test]
    fn a_restricted_visibility_still_leads_to_the_import() {
        assert_eq!(
            extract("pub(crate) use transport::serve_stdio;\n", Language::Rust)
                .imports
                .len(),
            1,
            "the parenthesised scope after pub must not hide the use"
        );
    }

    #[test]
    fn a_grouped_use_names_the_module_without_its_separator() {
        assert_eq!(
            extract("use super::support::{one, two};\n", Language::Rust).imports[0].specifier,
            "super::support"
        );
    }

    #[test]
    fn rust_declarations_carry_their_kind_and_visibility() {
        let facts = extract(
            "pub struct Engine;\npub(crate) fn build() {}\nfn private() {}\ntrait Run {}\n",
            Language::Rust,
        );
        let items = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.exported))
            .collect::<Vec<_>>();
        assert!(
            items.contains(&("Engine", DeclarationKind::Struct, true)),
            "got {items:?}"
        );
        assert!(
            items.contains(&("build", DeclarationKind::Function, true)),
            "got {items:?}"
        );
        assert!(
            items.contains(&("private", DeclarationKind::Function, false)),
            "got {items:?}"
        );
        assert!(
            items.contains(&("Run", DeclarationKind::Trait, true)),
            "got {items:?}"
        );
    }

    #[test]
    fn go_groups_imports_and_capitalisation_marks_export() {
        let source = "package main\n\nimport (\n\t\"fmt\"\n\t\"edgehawk.com/app/reader\"\n)\n\n\
             func Exported() {}\nfunc internal() {}\n";
        let facts = extract(source, Language::Go);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["fmt", "edgehawk.com/app/reader"],
            "a grouped import block yields one fact per path"
        );
        let items = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.exported))
            .collect::<Vec<_>>();
        assert!(items.contains(&("Exported", true)), "got {items:?}");
        assert!(items.contains(&("internal", false)), "got {items:?}");
    }

    #[test]
    fn java_methods_belong_to_their_class() {
        let source = "package com.x;\nimport com.x.Helper;\n\
             public class Service {\n\
             \x20 private final Helper helper = null;\n\
             \x20 public void run() {\n\
             \x20   items.forEach(item -> {});\n\
             \x20 }\n\
             \x20 private int score(String value) { return 1; }\n\
             }\n";
        let items = declared(source, Language::Java);
        assert!(
            items.iter().any(|(name, kind, owner)| name == "Service"
                && *kind == DeclarationKind::Class
                && owner.is_none()),
            "got {items:?}"
        );
        for method in ["run", "score"] {
            assert!(
                items.iter().any(|(name, kind, owner)| name == method
                    && *kind == DeclarationKind::Method
                    && owner.as_deref() == Some("Service")),
                "{method} must be a method of Service, got {items:?}"
            );
        }
        assert!(
            !items.iter().any(|(name, ..)| name == "forEach"),
            "a call chain inside a body is not a declaration, got {items:?}"
        );
    }

    #[test]
    fn solidity_contracts_own_their_functions_and_name_their_dependencies() {
        let source = "// SPDX-License-Identifier: MIT\n\
             pragma solidity ^0.8.20;\n\
             import \"./Ownable.sol\";\n\
             import {IERC20, SafeMath} from \"@openzeppelin/contracts/token/IERC20.sol\";\n\
             \n\
             contract Vault is Ownable {\n\
             \x20 event Deposited(address indexed who, uint256 amount);\n\
             \x20 function deposit(uint256 amount) public payable {\n\
             \x20   token.transferFrom(msg.sender, address(this), amount);\n\
             \x20 }\n\
             \x20 function _sweep() internal {}\n\
             }\n";
        let facts = extract(source, Language::Solidity);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["./Ownable.sol", "@openzeppelin/contracts/token/IERC20.sol"],
            "the names listed before `from` are bindings, not the path"
        );
        let items = declared(source, Language::Solidity);
        assert!(
            items.iter().any(|(name, kind, owner)| name == "Vault"
                && *kind == DeclarationKind::Class
                && owner.is_none()),
            "got {items:?}"
        );
        for method in ["deposit", "_sweep"] {
            assert!(
                items.iter().any(|(name, kind, owner)| name == method
                    && *kind == DeclarationKind::Function
                    && owner.as_deref() == Some("Vault")),
                "{method} belongs to Vault, got {items:?}"
            );
        }
        assert!(
            facts.references.iter().any(
                |call| call.name == "transferFrom" && call.receiver.as_deref() == Some("token")
            ),
            "a call through a receiver keeps the receiver"
        );
    }

    #[test]
    fn swift_members_belong_to_the_type_their_extension_names() {
        let source = "import Foundation\n\
             import UIKit\n\
             \n\
             public struct Engine {\n\
             \x20 let name: String\n\
             \x20 public func start() { boot() }\n\
             }\n\
             \n\
             extension Engine {\n\
             \x20 func restart() { start() }\n\
             }\n\
             \n\
             private func boot() {}\n";
        let facts = extract(source, Language::Swift);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["Foundation", "UIKit"]
        );
        let items = declared(source, Language::Swift);
        assert!(
            items.iter().any(|(name, kind, owner)| name == "start"
                && *kind == DeclarationKind::Function
                && owner.as_deref() == Some("Engine")),
            "got {items:?}"
        );
        assert!(
            items
                .iter()
                .any(|(name, _, owner)| name == "restart" && owner.as_deref() == Some("Engine")),
            "an extension names what its members belong to, got {items:?}"
        );
        assert!(
            !items.iter().any(|(name, ..)| name == "extension"),
            "and declares nothing itself, got {items:?}"
        );
        assert!(
            items
                .iter()
                .any(|(name, _, owner)| name == "boot" && owner.is_none()),
            "the file-level function is back outside, got {items:?}"
        );
    }

    #[test]
    fn a_rust_impl_gives_its_methods_an_owner() {
        let source = "struct Engine;\n\
             impl Engine {\n\
             \x20   pub fn start(&self) {}\n\
             }\n\
             impl Display for Engine {\n\
             \x20   fn fmt(&self) {}\n\
             }\n";
        let items = declared(source, Language::Rust);
        for method in ["start", "fmt"] {
            assert!(
                items
                    .iter()
                    .any(|(name, _, owner)| name == method && owner.as_deref() == Some("Engine")),
                "{method} belongs to Engine, not to the trait, got {items:?}"
            );
        }
    }

    #[test]
    fn c_functions_are_declarations_rather_than_calls_to_themselves() {
        // No keyword introduces a C function, so every definition fell through
        // to the call path: the graph gained a self-edge and lost the
        // declaration that dead-code analysis looks for.
        let source = "#include <stdio.h>\n\
             int add(int a, int b) { return a + b; }\n\
             static void run(void) { add(1, 2); }\n\
             int main(void) { run(); return 0; }\n";
        let facts = extract(source, Language::C);
        let declared = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.exported))
            .collect::<Vec<_>>();
        assert_eq!(
            declared,
            [
                ("add", DeclarationKind::Function, true),
                ("run", DeclarationKind::Function, false),
                ("main", DeclarationKind::Function, true),
            ],
            "a static function is file-local; the rest are linkable"
        );
        assert_eq!(
            facts
                .references
                .iter()
                .map(|reference| reference.name.as_str())
                .collect::<Vec<_>>(),
            ["add", "run"],
            "only the two real call sites, and no definition among them"
        );
    }

    #[test]
    fn an_include_does_not_swallow_the_line_beneath_it() {
        let facts = extract(
            "#include <stdio.h>\nint first(void) { return 0; }\n",
            Language::C,
        );
        assert_eq!(
            facts.imports.len(),
            1,
            "the include is one dependency, not a run-on statement"
        );
        assert!(
            facts.declarations.iter().any(|item| item.name == "first"),
            "the function under the include survives, got {:?}",
            facts.declarations
        );
    }

    #[test]
    fn an_out_of_line_cpp_definition_belongs_to_its_class() {
        let facts = extract(
            "int helper(int x) { return x; }\nvoid Engine::start() { helper(1); }\n",
            Language::Cpp,
        );
        let start = facts
            .declarations
            .iter()
            .find(|item| item.name == "start")
            .expect("the qualified definition is a declaration");
        assert_eq!(start.kind, DeclarationKind::Method);
        assert_eq!(start.owner.as_deref(), Some("Engine"));
    }

    #[test]
    fn a_type_argument_reaches_the_name_a_mapper_configures() {
        // An object-relational mapper names the entity in the type argument
        // and the table in the string one, so a framework layer needs both to
        // connect them - and the parenthesis does not follow the name.
        let facts = extract(
            "modelBuilder.Entity<Order>().ToTable(\"orders\");\nif (a < b && c > d) {}\n",
            Language::CSharp,
        );
        let seen = facts
            .references
            .iter()
            .map(|reference| {
                (
                    reference.name.as_str(),
                    reference.kind,
                    reference.string_arguments.clone(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            seen.contains(&("Order", ReferenceKind::Uses, Vec::new())),
            "the entity type is reachable, got {seen:?}"
        );
        assert!(
            seen.contains(&("ToTable", ReferenceKind::Call, vec!["orders".to_owned()])),
            "and so is the table it maps to, got {seen:?}"
        );
        assert!(
            !seen.iter().any(|(name, ..)| *name == "b"),
            "a comparison is not a type argument list, got {seen:?}"
        );
    }

    #[test]
    fn a_control_structure_is_not_a_function_definition() {
        let source = "int pick(int x) {\n\
             \x20 if (x) { return 1; }\n\
             \x20 else if (x > 2) { return 2; }\n\
             \x20 while (x) { x--; }\n\
             \x20 return helper(x);\n\
             }\n";
        let names = declared(source, Language::C)
            .into_iter()
            .map(|(name, ..)| name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["pick"],
            "an else-if has an identifier before it and still declares nothing"
        );
    }

    #[test]
    fn a_comment_never_declares_anything_in_any_brace_language() {
        for (source, language) in [
            (
                "// pub fn ghost() {}\n/* struct Ghost; */\npub fn real() {}\n",
                Language::Rust,
            ),
            ("// func Ghost() {}\nfunc Real() {}\n", Language::Go),
        ] {
            let items = declared(source, language);
            assert_eq!(
                items.len(),
                1,
                "only the real declaration counts: {items:?}"
            );
        }
    }
}
