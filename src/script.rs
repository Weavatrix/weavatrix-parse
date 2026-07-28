//! Structural extraction for JavaScript and TypeScript.
//!
//! The pass walks the token stream once, tracking brace depth to know which
//! declaration owns what. It reads the forms a repository graph is built from:
//! every import and re-export shape, declarations including class members,
//! and call sites with their receiver and string arguments.
//!
//! What it does not do is parse expressions. A call is recognised by an
//! identifier followed by `(`, not by building an expression tree, because no
//! consumer of these facts asks about precedence.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Reference, ReferenceKind, Span};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};

/// Extracts structural facts from one JavaScript or TypeScript source.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    Extractor {
        source,
        tokens: &tokens,
        facts: Facts::default(),
        scopes: Vec::new(),
        depth: 0,
    }
    .run()
}

/// A declaration whose body the walk is currently inside.
struct Scope {
    name: String,
    /// Depth of the body, once it opens. A declaration is recorded before its
    /// `{` is seen, so until then the scope is waiting and must not be closed
    /// by the very brace that opens it.
    depth: Option<i32>,
    /// Whether members declared directly inside are class members.
    class_body: bool,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    facts: Facts,
    scopes: Vec<Scope>,
    depth: i32,
}

impl Extractor<'_, '_> {
    fn run(mut self) -> Facts {
        let mut index = 0;
        while index < self.tokens.len() {
            self.close_scopes();
            index = self.step(index);
        }
        self.facts
    }

    fn text(&self, index: usize) -> &str {
        self.tokens
            .get(index)
            .map_or("", |token| token.text(self.source))
    }

    fn kind(&self, index: usize) -> Option<TokenKind> {
        self.tokens.get(index).map(|token| token.kind)
    }

    fn is(&self, index: usize, word: &str) -> bool {
        self.kind(index) == Some(TokenKind::Identifier) && self.text(index) == word
    }

    fn punct(&self, index: usize, mark: &str) -> bool {
        self.kind(index) == Some(TokenKind::Punctuation) && self.text(index) == mark
    }

    fn span(&self, start: usize, end: usize) -> Span {
        let first = &self.tokens[start.min(self.tokens.len() - 1)];
        let last = &self.tokens[end.min(self.tokens.len() - 1)];
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

    /// Binds the innermost waiting scope to the body that just opened.
    fn open_body(&mut self) {
        let depth = self.depth;
        if let Some(scope) = self.scopes.last_mut()
            && scope.depth.is_none()
        {
            scope.depth = Some(depth);
        }
    }

    /// Consumes one construct starting at `index`, returning the next index.
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
        if (self.is(index, "import") || self.is(index, "export"))
            && let Some(next) = self.module_statement(index)
        {
            return next;
        }
        if self.kind(index) == Some(TokenKind::Identifier) {
            if let Some(next) = self.declaration(index) {
                return next;
            }
            if let Some(next) = self.call(index) {
                return next;
            }
        }
        index + 1
    }

    /// `import ... from 'x'`, `import 'x'`, `import('x')`, `require('x')`,
    /// `export ... from 'x'`. Multi-line forms work because the walk is over
    /// tokens rather than lines.
    fn module_statement(&mut self, index: usize) -> Option<usize> {
        let exporting = self.is(index, "export");
        let mut cursor = index + 1;
        let mut type_only = self.is(cursor, "type");
        if type_only {
            cursor += 1;
        }
        // `export function|class|const ...` is a declaration, not a module
        // statement; let the declaration path handle it.
        if exporting && !self.leads_to_from(cursor) {
            return None;
        }
        if !exporting && self.punct(cursor, "(") {
            // Dynamic import: `import('x')`.
            let specifier = self.string_at(cursor + 1)?;
            self.facts.imports.push(Import {
                specifier,
                span: self.span(index, cursor + 1),
                type_only: false,
                reexport: false,
            });
            return Some(cursor + 2);
        }
        let mut scan = cursor;
        let limit = (index + 512).min(self.tokens.len());
        while scan < limit {
            if self.kind(scan) == Some(TokenKind::String) {
                let specifier = self.string_at(scan)?;
                // `{ type X }` marks a single specifier as type-only too.
                type_only = type_only || self.braced_types_only(cursor, scan);
                self.facts.imports.push(Import {
                    specifier,
                    span: self.span(index, scan),
                    type_only,
                    reexport: exporting,
                });
                return Some(scan + 1);
            }
            if self.punct(scan, ";") || self.punct(scan, "{") && !exporting && scan > cursor + 1 {
                break;
            }
            scan += 1;
        }
        None
    }

    /// Whether a `export ...` statement reaches a `from` clause before its end.
    fn leads_to_from(&self, index: usize) -> bool {
        let limit = (index + 512).min(self.tokens.len());
        let mut scan = index;
        while scan < limit {
            if self.is(scan, "from") {
                return true;
            }
            if self.punct(scan, ";") || self.punct(scan, "=") {
                return false;
            }
            scan += 1;
        }
        false
    }

    /// Whether every named specifier between `start` and `end` is type-only.
    fn braced_types_only(&self, start: usize, end: usize) -> bool {
        let mut scan = start;
        let mut inside = false;
        let mut names = 0_usize;
        let mut typed = 0_usize;
        while scan < end {
            if self.punct(scan, "{") {
                inside = true;
            } else if self.punct(scan, "}") {
                inside = false;
            } else if inside && self.kind(scan) == Some(TokenKind::Identifier) {
                if self.text(scan) == "type" {
                    typed += 1;
                } else if !self.is(scan, "as") {
                    names += 1;
                }
            }
            scan += 1;
        }
        names > 0 && typed >= names
    }

    fn string_at(&self, index: usize) -> Option<String> {
        if self.kind(index) != Some(TokenKind::String) {
            return None;
        }
        let raw = self.text(index);
        let trimmed = raw
            .strip_prefix(['"', '\'', '`'])
            .and_then(|value| value.strip_suffix(['"', '\'', '`']))
            .unwrap_or(raw);
        Some(trimmed.to_owned())
    }

    /// Declarations in every form the language writes them.
    fn declaration(&mut self, index: usize) -> Option<usize> {
        let mut cursor = index;
        let mut exported = false;
        if self.is(cursor, "export") {
            exported = true;
            cursor += 1;
            if self.is(cursor, "default") {
                cursor += 1;
            }
        }
        for keyword in [
            "async", "declare", "abstract", "static", "public", "private",
        ] {
            if self.is(cursor, keyword) {
                cursor += 1;
            }
        }
        let kind = match self.text(cursor) {
            "function" => DeclarationKind::Function,
            "class" => DeclarationKind::Class,
            "interface" => DeclarationKind::Interface,
            "enum" => DeclarationKind::Enum,
            "type" => DeclarationKind::TypeAlias,
            "const" => DeclarationKind::Constant,
            "let" | "var" => DeclarationKind::Variable,
            // Not a keyword-introduced declaration: the name itself may still
            // declare a class member, and `cursor` has already skipped the
            // modifiers that preceded it.
            _ => return self.class_member(cursor, exported),
        };
        if self.kind(cursor) != Some(TokenKind::Identifier) {
            return None;
        }
        let name_index = if self.punct(cursor + 1, "*") {
            cursor + 2
        } else {
            cursor + 1
        };
        if self.kind(name_index) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(name_index).to_owned();
        // `const x = () => {}` declares a function, not a value.
        let kind = if matches!(kind, DeclarationKind::Constant | DeclarationKind::Variable)
            && self.is_arrow_function(name_index)
        {
            DeclarationKind::Function
        } else {
            kind
        };
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind,
            span: self.span(index, name_index),
            owner: self.owner(),
            exported,
        });
        if matches!(
            kind,
            DeclarationKind::Class | DeclarationKind::Interface | DeclarationKind::Enum
        ) {
            self.scopes.push(Scope {
                name,
                depth: None,
                class_body: true,
            });
        } else if matches!(kind, DeclarationKind::Function) {
            self.scopes.push(Scope {
                name,
                depth: None,
                class_body: false,
            });
        }
        Some(name_index + 1)
    }

    /// Whether the initializer is an arrow function. `=>` is two punctuation
    /// tokens, so the pair is matched rather than the text.
    fn is_arrow_function(&self, name_index: usize) -> bool {
        let limit = (name_index + 64).min(self.tokens.len());
        let mut scan = name_index + 1;
        while scan < limit {
            if self.punct(scan, "=") && self.punct(scan + 1, ">") {
                return true;
            }
            if self.punct(scan, ";") || self.punct(scan, "{") {
                return false;
            }
            scan += 1;
        }
        false
    }

    /// A method or field written directly inside a class body.
    fn class_member(&mut self, index: usize, exported: bool) -> Option<usize> {
        let inside_class = self.scopes.last().is_some_and(|scope| {
            scope.class_body && scope.depth.is_some_and(|depth| self.depth == depth)
        });
        if !inside_class || self.kind(index) != Some(TokenKind::Identifier) {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(name.as_str(), "return" | "if" | "for" | "while" | "switch") {
            return None;
        }
        // A method is a name followed by a parameter list; anything else
        // declared at class-body level is a field.
        let kind = if self.punct(index + 1, "(") || self.punct(index + 1, "<") {
            DeclarationKind::Method
        } else if self.punct(index + 1, ":") || self.punct(index + 1, "=") {
            DeclarationKind::Field
        } else {
            return None;
        };
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind,
            span: self.span(index, index),
            owner: self.owner(),
            exported,
        });
        if kind == DeclarationKind::Method {
            self.scopes.push(Scope {
                name,
                depth: None,
                class_body: false,
            });
            return Some(index + 1);
        }
        // A field initializer is still written at class-body depth, so
        // stepping through it would read `new Map()` as another member.
        Some(self.skip_initializer(index + 1))
    }

    /// Advances past a field initializer, stopping at the statement end that
    /// closes it. Nested braces and parens are stepped over as a unit.
    fn skip_initializer(&self, start: usize) -> usize {
        let mut scan = start;
        let mut nesting = 0_i32;
        let limit = (start + 512).min(self.tokens.len());
        while scan < limit {
            if self.punct(scan, "(") || self.punct(scan, "[") || self.punct(scan, "{") {
                nesting += 1;
            } else if self.punct(scan, ")") || self.punct(scan, "]") || self.punct(scan, "}") {
                if nesting == 0 {
                    return scan;
                }
                nesting -= 1;
            } else if nesting == 0 && self.punct(scan, ";") {
                return scan + 1;
            }
            scan += 1;
        }
        start
    }

    /// A call site, with the receiver it was written on.
    fn call(&mut self, index: usize) -> Option<usize> {
        if !self.punct(index + 1, "(") {
            return None;
        }
        let name = self.text(index).to_owned();
        if matches!(
            name.as_str(),
            "if" | "for" | "while" | "switch" | "catch" | "function" | "return" | "typeof"
        ) {
            return None;
        }
        let receiver = (self.punct(index.wrapping_sub(1), ".")
            && self.kind(index.wrapping_sub(2)) == Some(TokenKind::Identifier))
        .then(|| self.text(index - 2).to_owned());
        let mut arguments = Vec::new();
        let mut scan = index + 2;
        let mut depth = 1_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if depth == 1
                && self.kind(scan) == Some(TokenKind::String)
                && let Some(value) = self.string_at(scan)
            {
                arguments.push(value);
            }
            scan += 1;
        }
        // `require('x')` is how CommonJS imports, so it is an import as well
        // as a call; recording only the call would lose the dependency.
        if name == "require"
            && receiver.is_none()
            && let Some(specifier) = arguments.first()
        {
            self.facts.imports.push(Import {
                specifier: specifier.clone(),
                span: self.span(index, index),
                type_only: false,
                reexport: false,
            });
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
    /// React files are the ones where the lexer's assumptions are most
    /// fragile: `<` and `>` surround markup rather than comparing, and a
    /// slash inside JSX text must stay a division rather than opening a
    /// regular expression that would swallow the rest of the file.
    #[test]
    fn jsx_does_not_derail_the_token_stream() {
        use crate::syntax::Language;
        use crate::token::{TokenKind, tokenize};

        let source = "import React from 'react';\n\
             import { Button } from './Button';\n\
             export function Panel({ items, onPick }) {\n\
             \x20 const ratio = items.length / 2;\n\
             \x20 return (\n\
             \x20   <div className=\"panel\" data-count={items.length}>\n\
             \x20     {items.map((item) => (\n\
             \x20       <Button key={item.id} onClick={() => onPick(item)}>\n\
             \x20         {item.label} / {ratio}\n\
             \x20       </Button>\n\
             \x20     ))}\n\
             \x20   </div>\n\
             \x20 );\n\
             }\n\
             export const Footer = () => <footer>done</footer>;\n";
        let tokens = tokenize(source, Language::TypeScript);
        assert_eq!(
            tokens
                .iter()
                .map(|token| token.text(source))
                .collect::<String>(),
            source,
            "the stream must still reproduce the file"
        );
        assert!(
            !tokens
                .iter()
                .any(|token| matches!(token.kind, TokenKind::Regex | TokenKind::Unterminated)),
            "no division in JSX may be read as a regular expression"
        );
        let facts = super::extract(source, Language::TypeScript);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            ["react", "./Button"]
        );
        for name in ["Panel", "Footer"] {
            assert!(
                facts.declarations.iter().any(|item| item.name == name),
                "{name} must survive the markup, got {:?}",
                facts
                    .declarations
                    .iter()
                    .map(|item| item.name.as_str())
                    .collect::<Vec<_>>()
            );
        }
    }

    use super::extract;
    use crate::facts::DeclarationKind;
    use crate::syntax::Language;

    fn specifiers(source: &str) -> Vec<(String, bool, bool)> {
        extract(source, Language::TypeScript)
            .imports
            .into_iter()
            .map(|import| (import.specifier, import.type_only, import.reexport))
            .collect()
    }

    #[test]
    fn reads_every_module_form_including_multi_line() {
        let source = "import defaultExport from './a';\n\
             import {\n  first,\n  second,\n} from './b';\n\
             import type { Shape } from './c';\n\
             import { type Only } from './d';\n\
             import * as everything from './e';\n\
             import './f';\n\
             const legacy = require('./g');\n\
             const lazy = await import('./h');\n\
             export { thing } from './i';\n\
             export * from './j';\n";
        assert_eq!(
            specifiers(source),
            [
                ("./a".to_owned(), false, false),
                ("./b".to_owned(), false, false),
                ("./c".to_owned(), true, false),
                ("./d".to_owned(), true, false),
                ("./e".to_owned(), false, false),
                ("./f".to_owned(), false, false),
                ("./g".to_owned(), false, false),
                ("./h".to_owned(), false, false),
                ("./i".to_owned(), false, true),
                ("./j".to_owned(), false, true),
            ],
            "a multi-line import is one fact, and type-only is distinguished"
        );
    }

    #[test]
    fn a_comment_or_string_never_becomes_a_fact() {
        let source = "// import { fake } from './nope';\n\
             const text = \"import { alsoFake } from './nope2'\";\n\
             /* app.get('/commented-route', handler); */\n\
             import { real } from './yes';\n\
             app.get('/real-route', handler);\n";
        assert_eq!(specifiers(source), [("./yes".to_owned(), false, false)]);
        let facts = extract(source, Language::TypeScript);
        let routes = facts
            .references
            .iter()
            .flat_map(|call| call.string_arguments.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            routes,
            ["/real-route"],
            "the commented route is not a call argument"
        );
    }

    #[test]
    fn class_bodies_yield_methods_and_fields_with_their_owner() {
        let source = "export class Service {\n\
             \x20 private cache = new Map();\n\
             \x20 readonly limit: number = 10;\n\
             \x20 async run(input: string) {\n\
             \x20   return this.helper(input);\n\
             \x20 }\n\
             \x20 helper(value: string) { return value; }\n\
             }\n";
        let facts = extract(source, Language::TypeScript);
        let declared = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
            .collect::<Vec<_>>();
        assert!(
            declared.contains(&("Service", DeclarationKind::Class, None)),
            "got {declared:?}"
        );
        assert!(
            declared.contains(&("run", DeclarationKind::Method, Some("Service"))),
            "a class method is a declaration owned by its class, got {declared:?}"
        );
        assert!(
            declared.contains(&("helper", DeclarationKind::Method, Some("Service"))),
            "got {declared:?}"
        );
        assert!(
            declared.contains(&("cache", DeclarationKind::Field, Some("Service"))),
            "got {declared:?}"
        );
        let call = facts
            .references
            .iter()
            .find(|call| call.name == "helper")
            .expect("the call inside run is recorded");
        assert_eq!(call.receiver.as_deref(), Some("this"));
        assert_eq!(call.owner.as_deref(), Some("run"));
    }

    #[test]
    fn arrow_constants_are_functions_and_plain_constants_are_not() {
        let source = "export const load = async () => { return 1; };\nconst limit = 10;\n";
        let facts = extract(source, Language::TypeScript);
        let kinds = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.exported))
            .collect::<Vec<_>>();
        assert!(
            kinds.contains(&("load", DeclarationKind::Function, true)),
            "got {kinds:?}"
        );
        assert!(
            kinds.contains(&("limit", DeclarationKind::Constant, false)),
            "got {kinds:?}"
        );
    }
}
