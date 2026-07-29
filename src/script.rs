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

use crate::facts::{
    Declaration, DeclarationKind, Facts, Import, ImportBinding, Reference, ReferenceKind, Span,
};
use crate::syntax::Language;
use crate::token::{Mode, Token, TokenKind, Tokenizer};
use std::collections::BTreeMap;

/// Extracts structural facts from one JavaScript or TypeScript source.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let tokens = Tokenizer::new(source, language)
        .mode(Mode::Lite)
        .collect::<Vec<_>>();
    Extractor {
        source,
        tokens: &tokens,
        language,
        facts: Facts::default(),
        scopes: Vec::new(),
        import_bindings: BTreeMap::new(),
        depth: 0,
        paren_depth: 0,
        bracket_depth: 0,
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
    /// Whether members declared directly inside are class or object members.
    member_body: bool,
    /// Classes declare fields; object literals only contribute named methods.
    fields: bool,
    /// Parenthesis/bracket nesting at the member body's opening brace.
    paren_depth: i32,
    bracket_depth: i32,
}

struct Extractor<'source, 'tokens> {
    source: &'source str,
    tokens: &'tokens [Token],
    language: Language,
    facts: Facts,
    scopes: Vec<Scope>,
    import_bindings: BTreeMap<String, (String, bool, String)>,
    depth: i32,
    paren_depth: i32,
    bracket_depth: i32,
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
            && scope.paren_depth == self.paren_depth
            && scope.bracket_depth == self.bracket_depth
        {
            scope.depth = Some(depth);
            scope.paren_depth = self.paren_depth;
            scope.bracket_depth = self.bracket_depth;
        }
    }

    /// Consumes one construct starting at `index`, returning the next index.
    fn step(&mut self, index: usize) -> usize {
        if self.punct(index, "{") {
            let object_owner = self.object_literal_owner(index);
            let waiting_scope = self
                .scopes
                .last()
                .is_some_and(|scope| scope.depth.is_none());
            self.depth += 1;
            self.open_body();
            if !waiting_scope && let Some(name) = object_owner {
                self.scopes.push(Scope {
                    name,
                    depth: Some(self.depth),
                    member_body: true,
                    fields: false,
                    paren_depth: self.paren_depth,
                    bracket_depth: self.bracket_depth,
                });
            }
            return index + 1;
        }
        if self.punct(index, "}") {
            self.depth -= 1;
            return index + 1;
        }
        if self.punct(index, "(") {
            self.paren_depth += 1;
            return index + 1;
        }
        if self.punct(index, ")") {
            self.paren_depth -= 1;
            return index + 1;
        }
        if self.punct(index, "[") {
            self.bracket_depth += 1;
            return index + 1;
        }
        if self.punct(index, "]") {
            self.bracket_depth -= 1;
            return index + 1;
        }
        if (self.is(index, "import") || self.is(index, "export"))
            && let Some(next) = self.module_statement(index)
        {
            return next;
        }
        if self.kind(index) == Some(TokenKind::String) {
            self.template_references(index);
            if let Some(next) = self.route_table(index) {
                return next;
            }
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

    /// Calls inside `${...}` are program expressions even though the lossless
    /// tokenizer deliberately keeps the complete template as one string
    /// token. Extract each balanced expression separately and relocate its
    /// references to the original file. Literal template text is never parsed
    /// as code.
    fn template_references(&mut self, index: usize) {
        let token = &self.tokens[index];
        let template = token.text(self.source);
        if !template.starts_with('`') {
            return;
        }
        let owner = self.owner();
        for (start, end) in template_interpolation_ranges(template, self.language) {
            let Some(expression) = template.get(start..end) else {
                continue;
            };
            let base = token.start + start;
            let (base_line, base_column) = position_at(self.source, base);
            let mut references = extract(expression, self.language).references;
            for reference in &mut references {
                reference.span.start += base;
                reference.span.end += base;
                reference.span.line = base_line.saturating_add(reference.span.line - 1);
                if reference.span.line == base_line {
                    reference.span.column = base_column.saturating_add(reference.span.column - 1);
                }
                reference.span.end_line = base_line.saturating_add(reference.span.end_line - 1);
                if reference.span.end_line == base_line {
                    reference.span.end_column =
                        base_column.saturating_add(reference.span.end_column - 1);
                }
                reference.owner.clone_from(&owner);
            }
            self.facts.references.extend(references);
        }
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
            return self.local_reexport(index, cursor);
        }
        if !exporting && self.punct(cursor, "(") {
            // Dynamic import: `import('x')`.
            let specifier = self.string_at(cursor + 1)?;
            self.facts.imports.push(Import {
                specifier,
                span: self.span(index, cursor + 1),
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
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
                let bindings = self.clause_bindings(cursor, scan);
                let names = bindings
                    .iter()
                    .map(|binding| binding.local.clone())
                    .collect::<Vec<_>>();
                if !exporting {
                    for binding in &bindings {
                        self.import_bindings.insert(
                            binding.local.clone(),
                            (specifier.clone(), type_only, binding.imported.clone()),
                        );
                    }
                }
                self.facts.imports.push(Import {
                    specifier,
                    span: self.span(index, scan),
                    type_only,
                    reexport: exporting,
                    names,
                    bindings,
                });
                return Some(scan + 1);
            }
            // Between the keyword and the specifier, an import clause holds
            // only names and the punctuation that groups them. Anything else
            // means this was never an import statement, and the scan must stop
            // rather than run on into the code beneath it.
            //
            // The previous rule broke at any `{` that was not the second
            // token, which silently lost every `import Default, { named }
            // from 'x'` - the one shape that has a name before the brace.
            if self.punct(scan, ";") {
                break;
            }
            if self.kind(scan) == Some(TokenKind::Punctuation)
                && !matches!(self.text(scan), "{" | "}" | "," | "*")
            {
                break;
            }
            scan += 1;
        }
        None
    }

    /// `export { importedName }` forwards the module that originally bound the
    /// local name even though this statement has no `from` clause of its own.
    fn local_reexport(&mut self, start: usize, open: usize) -> Option<usize> {
        if !self.punct(open, "{") {
            return None;
        }
        let limit = (open + 512).min(self.tokens.len());
        let mut cursor = open + 1;
        let mut by_target = BTreeMap::<(String, bool), Vec<ImportBinding>>::new();
        while cursor < limit && !self.punct(cursor, "}") {
            if self.kind(cursor) == Some(TokenKind::Identifier)
                && !matches!(self.text(cursor), "as" | "type")
                && !self.is(cursor.wrapping_sub(1), "as")
                && let Some((target, type_only, imported)) =
                    self.import_bindings.get(self.text(cursor))
            {
                let local = if self.is(cursor + 1, "as")
                    && self.kind(cursor + 2) == Some(TokenKind::Identifier)
                {
                    self.text(cursor + 2).to_owned()
                } else {
                    self.text(cursor).to_owned()
                };
                by_target
                    .entry((target.clone(), *type_only))
                    .or_default()
                    .push(ImportBinding {
                        imported: imported.clone(),
                        local,
                    });
            }
            cursor += 1;
        }
        if !self.punct(cursor, "}") {
            return None;
        }
        for ((specifier, type_only), bindings) in by_target {
            let names = bindings
                .iter()
                .map(|binding| binding.local.clone())
                .collect();
            self.facts.imports.push(Import {
                specifier,
                span: self.span(start, cursor),
                type_only,
                reexport: true,
                names,
                bindings,
            });
        }
        Some(cursor + 1)
    }

    /// A route table written as an object: `{ '/items': { POST: handler } }`.
    ///
    /// The path is a key rather than an argument, so no call site mentions it
    /// and the ordinary call path cannot see it - but it registers a route
    /// exactly as `router.post('/items', handler)` does.
    fn route_table(&mut self, index: usize) -> Option<usize> {
        let path = self.string_at(index)?;
        if !path.starts_with('/') || !self.punct(index + 1, ":") || !self.punct(index + 2, "{") {
            return None;
        }
        let limit = (index + 128).min(self.tokens.len());
        let mut cursor = index + 3;
        let mut depth = 1_i32;
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "{") {
                depth += 1;
            } else if self.punct(cursor, "}") {
                depth -= 1;
            } else if depth == 1
                && self.kind(cursor) == Some(TokenKind::Identifier)
                && self.punct(cursor + 1, ":")
                && is_method(self.text(cursor))
            {
                self.facts.references.push(Reference {
                    name: self.text(cursor).to_owned(),
                    kind: ReferenceKind::Call,
                    receiver: None,
                    span: self.span(index, cursor),
                    owner: self.owner(),
                    string_arguments: vec![path.clone()],
                    name_arguments: Vec::new(),
                });
            }
            cursor += 1;
        }
        Some(cursor)
    }

    /// The exported and local names an import clause binds.
    fn clause_bindings(&self, start: usize, end: usize) -> Vec<ImportBinding> {
        let mut bindings = Vec::new();
        let mut scan = start;
        let mut braced = false;
        let mut default_seen = false;
        while scan < end {
            if self.punct(scan, "{") {
                braced = true;
                scan += 1;
                continue;
            }
            if self.punct(scan, "}") {
                braced = false;
                scan += 1;
                continue;
            }
            if self.punct(scan, "*")
                && self.is(scan + 1, "as")
                && self.kind(scan + 2) == Some(TokenKind::Identifier)
            {
                bindings.push(ImportBinding {
                    imported: "*".to_owned(),
                    local: self.text(scan + 2).to_owned(),
                });
                scan += 3;
                continue;
            }
            if self.kind(scan) == Some(TokenKind::Identifier) {
                let text = self.text(scan);
                if matches!(text, "from" | "type" | "as") || self.is(scan.wrapping_sub(1), "as") {
                    scan += 1;
                    continue;
                }
                if self.is(scan + 1, "as") && self.kind(scan + 2) == Some(TokenKind::Identifier) {
                    bindings.push(ImportBinding {
                        imported: text.to_owned(),
                        local: self.text(scan + 2).to_owned(),
                    });
                    scan += 3;
                    continue;
                }
                if braced {
                    bindings.push(ImportBinding {
                        imported: text.to_owned(),
                        local: text.to_owned(),
                    });
                } else if !default_seen {
                    bindings.push(ImportBinding {
                        imported: "default".to_owned(),
                        local: text.to_owned(),
                    });
                    default_seen = true;
                }
            }
            scan += 1;
        }
        bindings
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
                member_body: true,
                fields: true,
                paren_depth: self.paren_depth,
                bracket_depth: self.bracket_depth,
            });
        } else if matches!(kind, DeclarationKind::Function) {
            self.scopes.push(Scope {
                name,
                depth: None,
                member_body: false,
                fields: false,
                paren_depth: self.paren_depth,
                bracket_depth: self.bracket_depth,
            });
        }
        Some(name_index + 1)
    }

    /// Whether the initializer is an arrow function. `=>` is two punctuation
    /// tokens, so the pair is matched rather than the text.
    fn is_arrow_function(&self, name_index: usize) -> bool {
        let limit = (name_index + 64).min(self.tokens.len());
        let mut scan = name_index + 1;
        let mut nesting = 0_i32;
        let mut assignment = false;
        let mut value_started = false;
        let declaration_line = self.tokens[name_index].line;
        while scan < limit {
            if nesting == 0 && self.punct(scan, "=") && self.punct(scan + 1, ">") {
                return true;
            }
            if nesting == 0
                && self.tokens[scan].line > declaration_line
                && assignment
                && value_started
            {
                return false;
            }
            if nesting == 0 && self.punct(scan, "=") {
                assignment = true;
                scan += 1;
                continue;
            }
            if self.punct(scan, "(") || self.punct(scan, "[") {
                nesting += 1;
            } else if self.punct(scan, ")") || self.punct(scan, "]") {
                nesting -= 1;
            } else if self.punct(scan, ";") || self.punct(scan, "{") {
                return false;
            }
            if assignment {
                value_started = true;
            }
            scan += 1;
        }
        false
    }

    /// A method or field written directly inside a class body.
    fn class_member(&mut self, index: usize, exported: bool) -> Option<usize> {
        let inside_class = self.scopes.last().is_some_and(|scope| {
            scope.member_body
                && scope.depth.is_some_and(|depth| self.depth == depth)
                && scope.paren_depth == self.paren_depth
                && scope.bracket_depth == self.bracket_depth
        });
        if !inside_class || self.kind(index) != Some(TokenKind::Identifier) {
            return None;
        }
        let previous = self.text(index.wrapping_sub(1));
        let starts_member = matches!(
            previous,
            "{" | "}"
                | ","
                | ";"
                | "public"
                | "private"
                | "protected"
                | "static"
                | "async"
                | "get"
                | "set"
                | "readonly"
                | "abstract"
                | "declare"
        );
        if !starts_member {
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
            if !self.scopes.last().is_some_and(|scope| scope.fields) {
                return None;
            }
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
                member_body: false,
                fields: false,
                paren_depth: self.paren_depth,
                bracket_depth: self.bracket_depth,
            });
            return Some(index + 1);
        }
        // A field initializer is still written at class-body depth, so
        // stepping through it would read `new Map()` as another member.
        Some(self.skip_initializer(index + 1))
    }

    fn object_literal_owner(&self, open: usize) -> Option<String> {
        if self.is(open.wrapping_sub(1), "return") {
            return self.owner();
        }
        if self.punct(open.wrapping_sub(1), "=")
            && self.kind(open.wrapping_sub(2)) == Some(TokenKind::Identifier)
        {
            return Some(self.text(open - 2).to_owned());
        }
        if self.punct(open.wrapping_sub(1), ":")
            && self.kind(open.wrapping_sub(2)) == Some(TokenKind::Identifier)
        {
            return Some(self.text(open - 2).to_owned());
        }
        // Object wrappers are commonly returned through `Object.freeze({...})`
        // or another constructor-like call. Walk only the current expression;
        // a preceding `return` keeps the methods owned by the enclosing
        // factory, while an assignment gives the object its binding name.
        if self.punct(open.wrapping_sub(1), "(") {
            let boundary = open.saturating_sub(24);
            let mut scan = open - 1;
            while scan > boundary {
                scan -= 1;
                if self.is(scan, "return") {
                    return self.owner();
                }
                if self.punct(scan, "=")
                    && self.kind(scan.wrapping_sub(1)) == Some(TokenKind::Identifier)
                {
                    return Some(self.text(scan - 1).to_owned());
                }
                if self.punct(scan, ";") || self.punct(scan, "{") || self.punct(scan, "}") {
                    break;
                }
            }
        }
        None
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
        let type_arguments = self.type_argument_span(index + 1);
        let open = index + 1 + type_arguments;
        if !self.punct(open, "(") {
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
        let mut names = Vec::new();
        let mut scan = open + 1;
        let mut depth = 1_i32;
        let mut nested = 0_i32;
        let limit = (index + 256).min(self.tokens.len());
        while scan < limit && depth > 0 {
            if self.punct(scan, "(") {
                depth += 1;
            } else if self.punct(scan, ")") {
                depth -= 1;
            } else if self.punct(scan, "{") || self.punct(scan, "[") {
                nested += 1;
            } else if self.punct(scan, "}") || self.punct(scan, "]") {
                nested -= 1;
            } else if depth == 1
                && nested == 0
                && self.kind(scan) == Some(TokenKind::String)
                && let Some(value) = self.string_at(scan)
            {
                arguments.push(value);
            } else if depth == 1 && nested == 0 && self.kind(scan) == Some(TokenKind::Identifier) {
                // A bare name passed as an argument: the router in
                // `app.use("/api", router)`, the handler in `app.get(path, h)`.
                // A member access contributes only its root, because that is
                // the binding an importer can resolve.
                if !self.punct(scan.wrapping_sub(1), ".") {
                    names.push(self.text(scan).to_owned());
                }
            }
            scan += 1;
        }
        // `require('x')` is how CommonJS imports, so it is an import as well
        // as a call; recording only the call would lose the dependency.
        if name == "require"
            && receiver.is_none()
            && let Some(specifier) = arguments.first()
        {
            // `const router = require('./x')` binds the module to a name, and
            // a mount written later refers to it by that name and nothing
            // else.
            let bound = if self.punct(index.wrapping_sub(1), "=")
                && self.kind(index.wrapping_sub(2)) == Some(TokenKind::Identifier)
            {
                vec![self.text(index - 2).to_owned()]
            } else {
                Vec::new()
            };
            self.facts.imports.push(Import {
                specifier: specifier.clone(),
                span: self.span(index, index),
                type_only: false,
                reexport: false,
                bindings: bound
                    .iter()
                    .map(|local| ImportBinding {
                        imported: "*".to_owned(),
                        local: local.clone(),
                    })
                    .collect(),
                names: bound,
            });
        }
        self.facts.references.push(Reference {
            kind: ReferenceKind::Call,
            name,
            receiver,
            span: self.span(index, index),
            owner: self.owner(),
            string_arguments: arguments,
            name_arguments: names,
        });
        Some(index + 1)
    }

    /// Length of a balanced TypeScript type-argument list before a call.
    fn type_argument_span(&self, index: usize) -> usize {
        if !self.punct(index, "<") {
            return 0;
        }
        let limit = (index + 64).min(self.tokens.len());
        let mut cursor = index + 1;
        let mut depth = 1_i32;
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "<") {
                depth += 1;
            } else if self.punct(cursor, ">") {
                depth -= 1;
            } else if depth == 1 && matches!(self.text(cursor), ";" | "{" | "}") {
                return 0;
            }
            cursor += 1;
        }
        if depth == 0 && self.punct(cursor, "(") {
            cursor - index
        } else {
            0
        }
    }
}

/// Whether a name is an HTTP method written as a route-table key.
fn is_method(name: &str) -> bool {
    matches!(
        name,
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS" | "ALL"
    )
}

/// Byte ranges of the expressions enclosed by `${...}` in one JavaScript
/// template token. A nested template is one token while matching the outer
/// expression, so braces in its text cannot close the expression early.
fn template_interpolation_ranges(template: &str, language: Language) -> Vec<(usize, usize)> {
    let bytes = template.as_bytes();
    let mut ranges = Vec::new();
    let mut cursor = usize::from(bytes.first() == Some(&b'`'));
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'\\' {
            cursor = (cursor + 2).min(bytes.len());
            continue;
        }
        if bytes[cursor] == b'`' {
            break;
        }
        if bytes[cursor] != b'$' || bytes[cursor + 1] != b'{' {
            cursor += 1;
            continue;
        }
        let expression_start = cursor + 2;
        let tail = &template[expression_start..];
        let tokens = Tokenizer::new(tail, language)
            .mode(Mode::Lite)
            .collect::<Vec<_>>();
        let mut depth = 1_i32;
        let mut expression_end = None;
        for token in tokens {
            if token.kind != TokenKind::Punctuation {
                continue;
            }
            match token.text(tail) {
                "{" => depth += 1,
                "}" => {
                    depth -= 1;
                    if depth == 0 {
                        expression_end = Some(expression_start + token.start);
                        cursor = expression_start + token.end;
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(expression_end) = expression_end else {
            break;
        };
        ranges.push((expression_start, expression_end));
    }
    ranges
}

fn position_at(source: &str, offset: usize) -> (u32, u32) {
    let prefix = source.get(..offset).unwrap_or(source);
    let line = u32::try_from(prefix.bytes().filter(|byte| *byte == b'\n').count())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    let column = u32::try_from(
        prefix
            .rsplit_once('\n')
            .map_or(prefix, |(_, suffix)| suffix)
            .chars()
            .count(),
    )
    .unwrap_or(u32::MAX)
    .saturating_add(1);
    (line, column)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_default_import_alongside_named_ones_is_still_an_import() {
        use crate::syntax::Language;

        let source = "import './sideEffect.js';\n\
             import express from 'express';\n\
             import logger, { logRequest, logAction } from './logger.js';\n\
             import * as tty from 'node:tty';\n\
             import type { Config } from './config';\n\
             export { helper } from './helper.js';\n\
             const meta = import.meta.url;\n";
        let facts = super::extract(source, Language::TypeScript);
        assert_eq!(
            facts
                .imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect::<Vec<_>>(),
            [
                "./sideEffect.js",
                "express",
                "./logger.js",
                "node:tty",
                "./config",
                "./helper.js",
            ],
            "a name before the brace must not end the clause, and import.meta \
             is not a module statement"
        );
    }

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
    use crate::facts::{DeclarationKind, ImportBinding, ReferenceKind};
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
        let source = "export const load = async () => { return 1; };\n\
             const multiline =\n(value) => value;\n\
             const limit = 10;\n";
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
            kinds.contains(&("multiline", DeclarationKind::Function, false)),
            "got {kinds:?}"
        );
        assert!(
            kinds.contains(&("limit", DeclarationKind::Constant, false)),
            "got {kinds:?}"
        );
    }

    #[test]
    fn regexes_and_collection_initializers_are_not_arrow_functions() {
        let source = "const SAFE_SCRIPT = /^(?:test(?::|$)|[^:]+:(?:test|check)(?::|$))/i\n\
             const UNSAFE_SHELL_ARG = /[\\0\\r\\n&|<>^%!`\\\"]/ \n\
             const byId = new Map((graph.nodes || []).map((node) => [String(node.id), node]))\n\
             const files = new Set((graph.nodes || []).filter((node) => node.id))\n\
             const adjacency = new Map([...files].map((file) => [file, new Set()]))\n";
        let facts = extract(source, Language::JavaScript);
        for name in [
            "SAFE_SCRIPT",
            "UNSAFE_SHELL_ARG",
            "byId",
            "files",
            "adjacency",
        ] {
            let declaration = facts
                .declarations
                .iter()
                .find(|item| item.name == name)
                .unwrap_or_else(|| panic!("missing {name}: {facts:?}"));
            assert_eq!(
                declaration.kind,
                DeclarationKind::Constant,
                "{name} is a value initializer"
            );
        }
    }

    #[test]
    fn exported_functions_and_returned_object_methods_are_declarations() {
        let source = "export function runCommand(command, args = [], options = {}) {}\n\
             export function createGate() {\n\
             \x20 return {\n\
             \x20   shouldShow({ force = false } = {}) { return force },\n\
             \x20   reset() {},\n\
             \x20 }\n\
             }\n\
             export function createClassifier() {\n\
             \x20 return { explain(path, options = {}) { return path } }\n\
             }\n";
        let facts = extract(source, Language::JavaScript);
        let declared = facts
            .declarations
            .iter()
            .map(|item| (item.name.as_str(), item.kind, item.owner.as_deref()))
            .collect::<Vec<_>>();
        assert!(
            declared.contains(&("runCommand", DeclarationKind::Function, None)),
            "got {declared:?}"
        );
        for (name, owner) in [
            ("shouldShow", "createGate"),
            ("reset", "createGate"),
            ("explain", "createClassifier"),
        ] {
            assert!(
                declared.contains(&(name, DeclarationKind::Method, Some(owner))),
                "missing {owner}.{name}; got {declared:?}"
            );
        }
    }

    #[test]
    fn exporting_an_imported_binding_keeps_its_origin() {
        let source =
            "import { safeRead, MAX_FILE_BYTES } from '../util.js';\nexport { safeRead };\n";
        let facts = extract(source, Language::JavaScript);
        let forwarded = facts
            .imports
            .iter()
            .find(|item| item.reexport)
            .expect("local export of imported binding");
        assert_eq!(forwarded.specifier, "../util.js");
        assert_eq!(forwarded.names, ["safeRead"]);
        assert_eq!(
            forwarded.bindings,
            [ImportBinding {
                imported: "safeRead".to_owned(),
                local: "safeRead".to_owned(),
            }]
        );
    }

    #[test]
    fn aliased_imports_preserve_original_and_local_names() {
        let facts = extract(
            "import Default, {\n\
             \x20 architectureViolation as violation,\n\
             \x20 matchComponentSelector as matches,\n\
             } from './architecture.js';\n\
             import * as catalog from './catalog.js';\n",
            Language::JavaScript,
        );
        let architecture = facts
            .imports
            .iter()
            .find(|item| item.specifier == "./architecture.js")
            .expect("architecture import");
        assert_eq!(architecture.names, ["Default", "violation", "matches"]);
        assert_eq!(
            architecture.bindings,
            [
                ImportBinding {
                    imported: "default".to_owned(),
                    local: "Default".to_owned(),
                },
                ImportBinding {
                    imported: "architectureViolation".to_owned(),
                    local: "violation".to_owned(),
                },
                ImportBinding {
                    imported: "matchComponentSelector".to_owned(),
                    local: "matches".to_owned(),
                },
            ]
        );
        let catalog = facts
            .imports
            .iter()
            .find(|item| item.specifier == "./catalog.js")
            .expect("namespace import");
        assert_eq!(
            catalog.bindings,
            [ImportBinding {
                imported: "*".to_owned(),
                local: "catalog".to_owned(),
            }]
        );
    }

    #[test]
    fn nested_template_text_never_becomes_a_call() {
        let source = r"function exactUsage(files) {
  return `${files ? ` in ${plural(files)} file(s)` : ''}`
}
";
        let facts = extract(source, Language::JavaScript);
        assert!(
            !facts
                .references
                .iter()
                .any(|reference| reference.name == "file"),
            "`file(s)` is literal template text, got {:?}",
            facts.references
        );
        let plural = facts
            .references
            .iter()
            .find(|reference| reference.name == "plural")
            .expect("call in a nested interpolation");
        assert_eq!(plural.kind, ReferenceKind::Call);
        assert_eq!(plural.owner.as_deref(), Some("exactUsage"));
        assert_eq!(plural.span.line, 2);
    }

    #[test]
    fn template_interpolations_keep_all_calls_and_exact_spans() {
        let source = r"function describe(blob, name, edge, graph) {
  const mentioned = new RegExp(`x${escRe(name)}y`).test(blob)
  return `${compileKind(edge) ? labelOf(graph, edge.id) : ''}`
}
";
        let facts = extract(source, Language::JavaScript);
        let calls = facts
            .references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Call)
            .collect::<Vec<_>>();
        for (name, line) in [
            ("RegExp", 2),
            ("escRe", 2),
            ("test", 2),
            ("compileKind", 3),
            ("labelOf", 3),
        ] {
            let reference = calls
                .iter()
                .find(|reference| reference.name == name)
                .unwrap_or_else(|| panic!("missing {name}, got {calls:?}"));
            assert_eq!(reference.span.line, line);
            assert_eq!(
                &source[reference.span.start..reference.span.end],
                name,
                "relocated span must name the original call"
            );
            assert_eq!(reference.owner.as_deref(), Some("describe"));
        }
    }

    #[test]
    fn nested_call_arguments_are_not_object_methods() {
        let source = r"function build(makeClient, session) {
  return {
    make: () => makeClient({ timeoutMs: Math.max(100, remaining(session)) }),
  }
}
";
        let facts = extract(source, Language::JavaScript);
        let remaining = facts
            .references
            .iter()
            .filter(|reference| {
                reference.name == "remaining" && reference.kind == ReferenceKind::Call
            })
            .collect::<Vec<_>>();
        assert_eq!(remaining.len(), 1, "got {:?}", facts.references);
        assert_eq!(remaining[0].span.line, 3);
        assert!(
            !facts.declarations.iter().any(|declaration| {
                declaration.name == "remaining" && declaration.kind == DeclarationKind::Method
            }),
            "a nested argument is not an object method: {:?}",
            facts.declarations
        );
    }

    #[test]
    fn default_object_parameter_is_not_the_function_body() {
        let source = r"export function runCommand(command, args = [], options = {}) {
  return spawn(command, args, { env: childProcessEnv(options.env || {}) })
}
";
        let facts = extract(source, Language::JavaScript);
        let declaration = facts
            .declarations
            .iter()
            .find(|declaration| declaration.name == "runCommand")
            .expect("exported function declaration");
        assert_eq!(declaration.kind, DeclarationKind::Function);
        assert!(declaration.exported);
        let environment = facts
            .references
            .iter()
            .find(|reference| reference.name == "childProcessEnv")
            .expect("call inside function body");
        assert_eq!(environment.owner.as_deref(), Some("runCommand"));
    }

    #[test]
    fn call_in_returned_object_property_is_retained() {
        let source = r"function withGraph(graph) {
  const root = mkdtempSync('prefix')
  const graphPath = join(root, 'graph.json')
  return {root, graphPath, graph: loadGraph(graphPath)}
}
";
        let facts = extract(source, Language::JavaScript);
        let load = facts
            .references
            .iter()
            .find(|reference| {
                reference.name == "loadGraph" && reference.kind == ReferenceKind::Call
            })
            .unwrap_or_else(|| panic!("missing loadGraph, got {facts:?}"));
        assert_eq!(load.owner.as_deref(), Some("withGraph"));
        assert_eq!(load.span.line, 4);
    }

    #[test]
    fn object_method_names_are_not_calls_but_their_bodies_are() {
        let source = r"function wrap(client) {
  return Object.freeze({
    fromUri(uri) { return client.normalizer.fromUri(uri) },
    kill() { client.kill() },
  })
}
";
        let facts = extract(source, Language::JavaScript);
        let calls = facts
            .references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Call)
            .collect::<Vec<_>>();
        assert_eq!(
            calls
                .iter()
                .filter(|reference| reference.name == "fromUri")
                .count(),
            1,
            "only the body call is a reference, got {calls:?}"
        );
        let from_uri = calls
            .iter()
            .find(|reference| reference.name == "fromUri")
            .expect("body call");
        assert_eq!(from_uri.receiver.as_deref(), Some("normalizer"));
        assert_eq!(from_uri.span.line, 3);
        assert!(
            from_uri.span.column > 30,
            "the call must point inside the body, got {:?}",
            from_uri.span
        );
        assert_eq!(
            calls
                .iter()
                .filter(|reference| reference.name == "kill")
                .count(),
            1,
            "only client.kill() is a call, got {calls:?}"
        );
    }

    #[test]
    fn typescript_generic_calls_keep_their_call_fact() {
        let facts = extract(
            "export async function loadUser() { return get<User>('/users/1'); }\n",
            Language::TypeScript,
        );
        let call = facts
            .references
            .iter()
            .find(|reference| reference.name == "get" && reference.kind == ReferenceKind::Call)
            .expect("generic call");
        assert_eq!(call.string_arguments, ["/users/1"]);
    }

    #[test]
    fn calls_inside_object_fields_and_nested_arguments_are_all_retained() {
        let source = "function score(entry, count, total) {\n\
             \x20 return {...entry, hotspotScore: round(Math.sqrt(entry.value))}\n\
             }\n\
             function pair(pairs, count, total) {\n\
             \x20 pairs.push({jaccard: round(count / total), lift: round(Math.max(count, total))})\n\
             }\n";
        let facts = extract(source, Language::JavaScript);
        let calls = facts
            .references
            .iter()
            .filter(|reference| reference.kind == ReferenceKind::Call)
            .map(|reference| (reference.name.as_str(), reference.span.line))
            .collect::<Vec<_>>();
        assert_eq!(
            calls.iter().filter(|(name, _)| *name == "round").count(),
            3,
            "every aliased round call must survive, got {calls:?}"
        );
        for expected in [
            ("round", 2),
            ("sqrt", 2),
            ("push", 5),
            ("round", 5),
            ("max", 5),
        ] {
            assert!(
                calls.contains(&expected),
                "missing {expected:?}; got {calls:?}"
            );
        }
        for false_method in ["round", "sqrt", "max"] {
            assert!(
                !facts.declarations.iter().any(|declaration| {
                    declaration.name == false_method && declaration.kind == DeclarationKind::Method
                }),
                "a call used as an object value is not a method declaration"
            );
        }
    }
}
