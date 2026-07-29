//! Structural extraction for the brace-scoped languages.
//!
//! Rust, Go, Java, C#, C, C++ and Solidity differ in which keyword introduces
//! a declaration and how a module is named, and agree on everything else:
//! braces open bodies, a name followed by a parameter list is callable, and a
//! call is an identifier followed by `(`. Those differences are tables, so one
//! walk serves all seven instead of seven near-identical scanners - and adding
//! the next such language costs a table, not a scanner.

use crate::facts::{
    Declaration, DeclarationKind, Facts, Import, ImportBinding, Reference, ReferenceKind, Span,
};
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
        language,
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
    /// Whether `const (...)` and `var (...)` contain one declaration spec per
    /// top-level line. This is Go syntax, not a generic braced-language rule.
    grouped_declarations: bool,
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
                grouped_declarations: false,
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
                grouped_declarations: false,
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
                grouped_declarations: true,
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
                grouped_declarations: false,
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
                grouped_declarations: false,
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
                grouped_declarations: false,
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
    test_only: bool,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
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

    fn test_only_at(&self, index: usize) -> bool {
        if self.language != Language::Rust {
            return false;
        }
        self.scopes.last().is_some_and(|scope| scope.test_only)
            || self.rust_test_attribute_before(index)
    }

    fn record_test_only_declaration(&mut self, test_only: bool, span: Span) {
        if test_only {
            self.facts.test_only_declarations.push(span);
        }
    }

    /// Reads only the contiguous Rust attributes immediately before an item.
    ///
    /// The lossless tokenizer has already removed comments and strings from
    /// syntax consideration, so a quoted `#[cfg(test)]` cannot classify code.
    fn rust_test_attribute_before(&self, index: usize) -> bool {
        let mut cursor = index;
        while cursor > 0 && self.punct(cursor - 1, "]") {
            let end = cursor - 1;
            let mut start = end;
            let mut depth = 1_i32;
            while start > 0 {
                start -= 1;
                if self.punct(start, "]") {
                    depth += 1;
                } else if self.punct(start, "[") {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            if depth != 0 || start == 0 || !self.punct(start - 1, "#") {
                break;
            }
            if self.rust_attribute_is_test(start + 1, end) {
                return true;
            }
            cursor = start - 1;
        }
        false
    }

    fn rust_attribute_is_test(&self, start: usize, end: usize) -> bool {
        let Some(first) =
            (start..end).find(|&index| self.kind(index) == Some(TokenKind::Identifier))
        else {
            return false;
        };
        if self.text(first) == "cfg" {
            let Some(open) = (first + 1..end).find(|&index| self.punct(index, "(")) else {
                return false;
            };
            return self.cfg_has_positive_test(open + 1, end, false);
        }
        let path_end = (first..end)
            .find(|&index| self.punct(index, "("))
            .unwrap_or(end);
        (first..path_end)
            .rfind(|&index| self.kind(index) == Some(TokenKind::Identifier))
            .is_some_and(|index| {
                matches!(
                    self.text(index),
                    "test" | "rstest" | "proptest" | "wasm_bindgen_test" | "test_case"
                )
            })
    }

    fn cfg_has_positive_test(&self, start: usize, end: usize, negated: bool) -> bool {
        let mut cursor = start;
        while cursor < end {
            if self.kind(cursor) == Some(TokenKind::Identifier) {
                if self.text(cursor) == "not" && self.punct(cursor + 1, "(") {
                    let close = self.matching_paren(cursor + 1, end);
                    if self.cfg_has_positive_test(cursor + 2, close, !negated) {
                        return true;
                    }
                    cursor = close.saturating_add(1);
                    continue;
                }
                if self.text(cursor) == "test" && !negated {
                    return true;
                }
            }
            cursor += 1;
        }
        false
    }

    fn matching_paren(&self, open: usize, end: usize) -> usize {
        let mut depth = 0_i32;
        for cursor in open..end {
            if self.punct(cursor, "(") {
                depth += 1;
            } else if self.punct(cursor, ")") {
                depth -= 1;
                if depth == 0 {
                    return cursor;
                }
            }
        }
        end
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
    // One pass keeps the shared statement boundaries beside the language
    // spellings; those fail-open limits are what prevent run-on imports.
    #[allow(clippy::too_many_lines)]
    fn import(&mut self, start: usize) -> Option<usize> {
        // `pub mod x;` is still a module dependency, so modifiers are stepped
        // over here exactly as a declaration would step over them.
        let mut index = start;
        // `pub use x::y;` forwards another module's surface to importers of
        // this one, exactly as `export ... from` does in JavaScript, so an
        // importer reaches through it transitively.
        let forwarding = self.rules.exported_keyword == Some(self.text(start));
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
        let mut bindings = Vec::new();
        // A parenthesised block lists several paths, as Go writes them.
        if self.punct(index + 1, "(") {
            let mut cursor = index + 2;
            let limit = (index + 512).min(self.tokens.len());
            while cursor < limit && !self.punct(cursor, ")") {
                if self.kind(cursor) == Some(TokenKind::String) {
                    let specifier = self.text(cursor).trim_matches(['"', '`']).to_owned();
                    let bindings = self.package_import_bindings(cursor, &specifier);
                    let names = bindings
                        .iter()
                        .map(|binding| binding.local.clone())
                        .collect();
                    self.facts.imports.push(Import {
                        specifier,
                        span: self.span(cursor, cursor),
                        type_only: false,
                        reexport: false,
                        names,
                        bindings,
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
                names: Vec::new(),
                bindings: Vec::new(),
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
                if word == "import"
                    && bindings.is_empty()
                    && self.text(cursor.wrapping_sub(1)) != "from"
                {
                    bindings = self.package_import_bindings(cursor, &specifier);
                }
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
                let mut close = cursor + 1;
                while close < limit && !self.punct(close, "}") {
                    close += 1;
                }
                bindings.extend(self.named_import_bindings(cursor + 1, close));
                if !specifier.is_empty() {
                    cursor = close.saturating_add(1);
                    break;
                }
                cursor = close.saturating_add(1);
                continue;
            }
            if word == "use"
                && self.text(cursor) == "as"
                && self.kind(cursor + 1) == Some(TokenKind::Identifier)
            {
                let imported = specifier
                    .trim_end_matches(':')
                    .rsplit("::")
                    .next()
                    .unwrap_or(specifier.as_str())
                    .to_owned();
                bindings.push(ImportBinding {
                    imported,
                    local: self.text(cursor + 1).to_owned(),
                });
                cursor += 2;
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
        if word == "use" && bindings.is_empty() {
            let imported = specifier
                .rsplit("::")
                .next()
                .unwrap_or(specifier.as_str())
                .to_owned();
            if imported != "*" {
                bindings.push(ImportBinding {
                    local: imported.clone(),
                    imported,
                });
            }
        }
        let names = bindings
            .iter()
            .map(|binding| binding.local.clone())
            .collect();
        self.facts.imports.push(Import {
            specifier,
            span: self.span(index, cursor.saturating_sub(1)),
            type_only: false,
            reexport: forwarding,
            names,
            bindings,
        });
        Some(cursor)
    }

    fn named_import_bindings(&self, start: usize, end: usize) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();
        let mut cursor = start;
        while cursor < end {
            if self.kind(cursor) != Some(TokenKind::Identifier)
                || matches!(self.text(cursor), "as" | "type")
                || self.text(cursor.wrapping_sub(1)) == "as"
            {
                cursor += 1;
                continue;
            }
            let imported = self.text(cursor).to_owned();
            let local = if self.text(cursor + 1) == "as"
                && self.kind(cursor + 2) == Some(TokenKind::Identifier)
            {
                self.text(cursor + 2).to_owned()
            } else {
                imported.clone()
            };
            bindings.push(ImportBinding { imported, local });
            cursor += 1;
        }
        bindings
    }

    fn package_import_bindings(&self, path: usize, specifier: &str) -> Vec<ImportBinding> {
        let imported = specifier
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(specifier)
            .to_owned();
        let previous = path.wrapping_sub(1);
        let same_line = self
            .tokens
            .get(previous)
            .is_some_and(|token| token.line == self.tokens[path].line);
        let local = if same_line
            && self.kind(previous) == Some(TokenKind::Identifier)
            && !matches!(self.text(previous), "import" | "from")
        {
            self.text(previous).to_owned()
        } else if same_line && self.punct(previous, ".") {
            "*".to_owned()
        } else {
            imported.clone()
        };
        if local == "_" {
            Vec::new()
        } else {
            vec![ImportBinding { imported, local }]
        }
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
        if keyword == "const" && self.punct(cursor.wrapping_sub(1), "*") {
            // Rust raw-pointer pointees (`*const T`) are types, not constant
            // declarations. The declaration walk sees every identifier, so
            // this boundary must be explicit.
            return None;
        }
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
        // Go groups declarations: `const ( A = 1\n B = 2 )` declares both, and
        // the keyword is followed by a parenthesis rather than by a name.
        if self.rules.grouped_declarations && self.punct(cursor + 1, "(") {
            return Some(self.grouped_declarations(index, cursor + 2, *kind));
        }
        let name_index = cursor + 1;
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(name_index).to_owned();
        // Go marks export by an initial capital rather than a keyword.
        let exported = exported || name.starts_with(char::is_uppercase);
        let test_only = self.test_only_at(index);
        let declaration_span = self.span(index, name_index);
        self.drop_waiting();
        self.record_test_only_declaration(test_only, declaration_span);
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: *kind,
            span: declaration_span,
            owner: self.owner(),
            exported,
        });
        self.heritage(name_index + 1, &name);
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
            test_only,
        });
        Some(name_index + 1)
    }

    /// What a type declares itself to derive from or satisfy.
    ///
    /// `extends` and `implements` are different edges and the graph keeps them
    /// apart; Go and Solidity write only one relation, so everything after
    /// their marker inherits.
    fn heritage(&mut self, start: usize, owner: &str) {
        let limit = (start + 48).min(self.tokens.len());
        let mut cursor = start;
        let mut kind = None;
        while cursor < limit && !self.punct(cursor, "{") && !self.punct(cursor, ";") {
            match self.text(cursor) {
                // Solidity writes `contract Vault is Ownable` for what Java
                // writes as `extends`.
                "extends" | "is" => kind = Some(ReferenceKind::Inherits),
                "implements" => kind = Some(ReferenceKind::Implements),
                _ => {
                    if let Some(kind) = kind
                        && self.kind(cursor) == Some(TokenKind::Identifier)
                        && !self.punct(cursor.wrapping_sub(1), ".")
                    {
                        self.facts.references.push(Reference {
                            name: self.text(cursor).to_owned(),
                            kind,
                            receiver: None,
                            span: self.span(cursor, cursor),
                            owner: Some(owner.to_owned()),
                            string_arguments: Vec::new(),
                            name_arguments: Vec::new(),
                        });
                    }
                }
            }
            cursor += 1;
        }
    }

    /// A field written inside a type body: `private String name = value;`.
    ///
    /// The name is the last one before the terminator, which is what makes a
    /// generic type such as `Map<String, Order> index;` name `index` rather
    /// than one of its type arguments.
    fn braced_field(&mut self, index: usize, exported: bool) -> Option<usize> {
        let limit = (index + 32).min(self.tokens.len());
        let mut cursor = index;
        let mut name = None;
        while cursor < limit {
            if self.punct(cursor, ";") || self.punct(cursor, "=") {
                break;
            }
            // A parenthesis or a brace means this was never a field.
            if self.punct(cursor, "(") || self.punct(cursor, "{") {
                return None;
            }
            if self.kind(cursor) == Some(TokenKind::Identifier) {
                name = Some((self.text(cursor).to_owned(), cursor));
            }
            cursor += 1;
        }
        let (name, at) = name?;
        // A lone name is a reference, not a declaration: a field needs a type
        // before it.
        if at == index {
            return None;
        }
        let declaration_span = self.span(index, at);
        let test_only = self.test_only_at(index);
        self.record_test_only_declaration(test_only, declaration_span);
        self.facts.declarations.push(Declaration {
            name,
            kind: DeclarationKind::Field,
            span: declaration_span,
            owner: self.owner(),
            exported,
        });
        Some(cursor)
    }

    /// Every name declared inside a `const (...)` or `var (...)` group.
    ///
    /// Each line of the group declares one name, so the first identifier on a
    /// line is the declaration and the rest is its value.
    fn grouped_declarations(&mut self, start: usize, open: usize, kind: DeclarationKind) -> usize {
        let limit = self.tokens.len();
        let mut cursor = open;
        let mut line = 0;
        let mut found = false;
        let mut closed = false;
        let mut parentheses = 1_u32;
        let mut braces = 0_u32;
        let mut brackets = 0_u32;
        while cursor < limit && parentheses != 0 {
            if self.punct(cursor, "(") {
                parentheses = parentheses.saturating_add(1);
                cursor += 1;
                continue;
            }
            if self.punct(cursor, ")") {
                parentheses = parentheses.saturating_sub(1);
                cursor += 1;
                if parentheses == 0 {
                    closed = true;
                }
                continue;
            }
            if self.punct(cursor, "{") {
                braces = braces.saturating_add(1);
                cursor += 1;
                continue;
            }
            if self.punct(cursor, "}") {
                braces = braces.saturating_sub(1);
                cursor += 1;
                continue;
            }
            if self.punct(cursor, "[") {
                brackets = brackets.saturating_add(1);
                cursor += 1;
                continue;
            }
            if self.punct(cursor, "]") {
                brackets = brackets.saturating_sub(1);
                cursor += 1;
                continue;
            }
            if parentheses == 1
                && braces == 0
                && brackets == 0
                && self.kind(cursor) == Some(TokenKind::Identifier)
                && self.tokens[cursor].line != line
                && !cursor.checked_sub(1).is_some_and(|previous| {
                    self.kind(previous) == Some(TokenKind::Punctuation)
                        && matches!(
                            self.text(previous),
                            "=" | ","
                                | "."
                                | "+"
                                | "-"
                                | "*"
                                | "/"
                                | "%"
                                | "&"
                                | "|"
                                | "^"
                                | "!"
                                | "<"
                                | ">"
                                | ":"
                        )
                })
            {
                line = self.tokens[cursor].line;
                let name = self.text(cursor).to_owned();
                let exported = name.starts_with(char::is_uppercase);
                let declaration_span = self.span(cursor, cursor);
                let test_only = self.test_only_at(cursor);
                self.record_test_only_declaration(test_only, declaration_span);
                self.facts.declarations.push(Declaration {
                    name,
                    kind,
                    span: declaration_span,
                    owner: self.owner(),
                    exported,
                });
                found = true;
            }
            // Group initializers still contain calls (`flag.String(...)`,
            // constructors, conversions). The declaration pre-pass must not
            // make those references disappear merely because it advances over
            // the complete parenthesised group.
            if self.kind(cursor) == Some(TokenKind::Identifier)
                && let Some(next) = self.call(cursor)
            {
                cursor = next;
                continue;
            }
            cursor += 1;
        }
        if closed && found {
            cursor
        } else {
            // Malformed or pathologically truncated input must not swallow the
            // remainder of the file.
            start + 1
        }
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
        let test_only = self.test_only_at(keyword);
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: true,
            test_only,
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
        let test_only = self.test_only_at(index);
        let declaration_span = self.span(index, index);
        self.record_test_only_declaration(test_only, declaration_span);
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: if owner.is_some() {
                DeclarationKind::Method
            } else {
                DeclarationKind::Function
            },
            span: declaration_span,
            owner,
            // C has no visibility keyword; a static function is file-local and
            // everything else is linkable.
            exported: exported || !self.is_static(index),
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: false,
            test_only,
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
        // An annotation is not a member. `@GetMapping("/stock")` and
        // `[HttpGet("/health")]` configure the member written beneath them,
        // and reading them as declarations both invents a method and loses
        // the route they carry.
        if self.punct(index.wrapping_sub(1), "@") || self.punct(index.wrapping_sub(1), "[") {
            return None;
        }
        // `Type name(` declares a method; the name is the token before `(`.
        // Reaching a terminator first means there is no parameter list, so
        // this is a field rather than a method - and the loop must leave the
        // decision to the field path instead of giving up here.
        let mut cursor = index;
        let limit = (index + 16).min(self.tokens.len());
        while cursor < limit && !self.punct(cursor + 1, "(") {
            if self.punct(cursor, ";") || self.punct(cursor, "{") || self.punct(cursor, "=") {
                return self.braced_field(index, exported);
            }
            cursor += 1;
        }
        // `private String name;` declares a field: a type, a name, and no
        // parameter list. The line scanner recorded these, so losing them
        // would be a regression rather than a simplification.
        if cursor >= limit || !self.punct(cursor + 1, "(") {
            return self.braced_field(index, exported);
        }
        if self.kind(cursor) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(cursor).to_owned();
        if matches!(name.as_str(), "if" | "for" | "while" | "switch" | "return") {
            return None;
        }
        let test_only = self.test_only_at(index);
        let declaration_span = self.span(index, cursor);
        self.record_test_only_declaration(test_only, declaration_span);
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Method,
            span: declaration_span,
            owner: self.owner(),
            exported,
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            type_body: false,
            test_only,
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
            "if" | "for" | "while" | "switch" | "match" | "return" | "catch" | "sizeof" | "fn"
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
                name_arguments: Vec::new(),
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
            name_arguments: Vec::new(),
        });
        Some(index + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::extract;
    use crate::facts::{DeclarationKind, ImportBinding, ReferenceKind};
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
    fn rust_test_attributes_classify_the_declaration_and_nested_scope() {
        let source = r#"
            fn production() {}
            #[cfg(not(test))]
            fn production_without_tests() {}
            #[cfg(any(test, feature = "test-support"))]
            #[allow(dead_code)]
            mod tests {
                #[test]
                fn embedded_test() {}
                fn helper_for_test() {}
            }
            #[tokio::test]
            async fn async_test() {}
            #[cfg(not(not(test)))]
            fn double_negated_test() {}
        "#;
        let facts = extract(source, Language::Rust);
        let test_only = |name: &str| {
            facts
                .declarations
                .iter()
                .find(|declaration| declaration.name == name)
                .is_some_and(|declaration| facts.declaration_is_test_only(declaration.span))
        };
        for production in ["production", "production_without_tests"] {
            assert!(
                !test_only(production),
                "{production} is available to production"
            );
        }
        for test in [
            "tests",
            "embedded_test",
            "helper_for_test",
            "async_test",
            "double_negated_test",
        ] {
            assert!(test_only(test), "{test} is test-only syntax");
        }
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
        let import = extract("use super::support::{one as first, two};\n", Language::Rust)
            .imports
            .remove(0);
        assert_eq!(import.specifier, "super::support");
        assert_eq!(import.names, ["first", "two"]);
        assert_eq!(
            import.bindings,
            [
                ImportBinding {
                    imported: "one".to_owned(),
                    local: "first".to_owned(),
                },
                ImportBinding {
                    imported: "two".to_owned(),
                    local: "two".to_owned(),
                },
            ]
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
    fn rust_function_pointer_types_do_not_invent_declarations_or_calls() {
        let facts = extract(
            "type Callback = unsafe extern \"system\" fn(\n\
             \x20   *mut c_void,\n\
             \x20   *const u16,\n\
             ) -> *mut c_void;\n",
            Language::Rust,
        );
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.name == "Callback" && item.kind == DeclarationKind::TypeAlias),
            "the actual alias must survive, got {:?}",
            facts.declarations
        );
        for false_positive in ["mut", "const", "u16", "c_void"] {
            assert!(
                !facts
                    .declarations
                    .iter()
                    .any(|item| item.name == false_positive),
                "{false_positive} is part of a pointer type, got {:?}",
                facts.declarations
            );
        }
        assert!(
            !facts
                .references
                .iter()
                .any(|reference| reference.name == "fn" && reference.kind == ReferenceKind::Call),
            "the function-pointer marker is a type, not a call"
        );
    }

    #[test]
    fn go_groups_imports_and_capitalisation_marks_export() {
        let source = "package main\n\nimport (\n\tf \"fmt\"\n\t\"edgehawk.com/app/reader\"\n)\n\n\
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
        assert_eq!(
            facts.imports[0].bindings,
            [ImportBinding {
                imported: "fmt".to_owned(),
                local: "f".to_owned(),
            }]
        );
        assert_eq!(
            facts.imports[1].bindings,
            [ImportBinding {
                imported: "reader".to_owned(),
                local: "reader".to_owned(),
            }]
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
    fn go_const_and_var_groups_declare_each_line() {
        let source = r#"package main
const (
    EventAdd = "added"
    eventDelete = "deleted"
)
var (
    endpoint = flag.String("endpoint", "/events", "endpoint")
    topics = []string{EventAdd, eventDelete}
)
"#;
        let facts = extract(source, Language::Go);
        let items = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.span.line))
            .collect::<Vec<_>>();
        for expected in [
            ("EventAdd", DeclarationKind::Constant, 3),
            ("eventDelete", DeclarationKind::Constant, 4),
            ("endpoint", DeclarationKind::Variable, 7),
            ("topics", DeclarationKind::Variable, 8),
        ] {
            assert!(
                items.contains(&expected),
                "missing {expected:?}; got {items:?}"
            );
        }
    }

    #[test]
    fn go_grouped_initializers_keep_their_call_references() {
        let facts = extract(
            "package main\nvar (\n flagName = config.String(\"name\")\n)\n",
            Language::Go,
        );
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.name == "flagName" && item.kind == DeclarationKind::Variable),
            "the grouped declaration must survive"
        );
        assert!(
            facts.references.iter().any(|reference| {
                reference.name == "String"
                    && reference.kind == ReferenceKind::Call
                    && reference.receiver.as_deref() == Some("config")
            }),
            "the initializer call must survive, got {:?}",
            facts.references
        );
    }

    #[test]
    fn go_grouped_values_do_not_turn_continuation_lines_into_declarations() {
        let source = r#"package main
var (
    config = Config{
        Name: "primary",
    }
    continued =
        buildValue
    next = 1
)
"#;
        let facts = extract(source, Language::Go);
        let names = facts
            .declarations
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>();
        for expected in ["config", "continued", "next"] {
            assert!(
                names.contains(&expected),
                "missing {expected}; got {names:?}"
            );
        }
        for false_positive in ["Name", "buildValue"] {
            assert!(
                !names.contains(&false_positive),
                "{false_positive} is an initializer expression, got {names:?}"
            );
        }
    }

    #[test]
    fn a_large_go_group_does_not_swallow_following_functions() {
        use std::fmt::Write as _;

        let mut source = String::from("package main\nvar (\n");
        for index in 0..1_100 {
            let _ = writeln!(source, "value{index} = {index}");
        }
        source.push_str(")\nfunc AfterGroup() {}\n");
        let facts = extract(&source, Language::Go);
        assert!(
            facts
                .declarations
                .iter()
                .any(|item| item.name == "AfterGroup" && item.kind == DeclarationKind::Function),
            "the closing group delimiter must return scanning to the following function"
        );
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
    fn java_route_annotations_keep_their_structural_owner_and_literal() {
        let source = "@RequestMapping(\"warehouse\")\n\
             public class Service {\n\
             \x20 @GetMapping(\"/stock\")\n\
             \x20 public void stock() {}\n\
             }\n";
        let facts = extract(source, Language::Java);
        let class_mapping = facts
            .calls()
            .find(|call| call.name == "RequestMapping")
            .expect("class mapping call");
        assert_eq!(class_mapping.owner, None);
        assert_eq!(class_mapping.string_arguments, ["warehouse"]);

        let method_mapping = facts
            .calls()
            .find(|call| call.name == "GetMapping")
            .expect("method mapping call");
        assert_eq!(method_mapping.owner.as_deref(), Some("Service"));
        assert_eq!(method_mapping.string_arguments, ["/stock"]);
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
