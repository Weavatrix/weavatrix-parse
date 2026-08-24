use super::{
    Declaration, DeclarationKind, Extractor, Language, Reference, ReferenceKind, Scope, TokenKind,
};

impl Extractor<'_, '_> {
    /// `impl Type`, `impl Trait for Type` and `extension Type` name what their
    /// members belong to without declaring anything themselves.
    ///
    /// The name is the last one before the brace, which is what makes
    /// `impl Display for Engine` belong to `Engine` rather than to `Display`.
    pub(super) fn open_named_scope(&mut self, keyword: usize) -> Option<usize> {
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
        if self.language == Language::Swift {
            self.swift_extension_heritage(keyword + 1, cursor, &name);
        }
        let test_only = self.test_only_at(keyword);
        self.scopes.push(Scope {
            name,
            depth: None,
            declaration: None,
            type_body: true,
            test_only,
        });
        Some(cursor)
    }

    /// `extension Engine: Equatable, Codable` names the protocols the members
    /// satisfy. The colon is not `implements`, so the shared heritage walk
    /// never sees it unless this pass records those types.
    fn swift_extension_heritage(&mut self, start: usize, end: usize, owner: &str) {
        let mut cursor = start;
        let mut active = false;
        let mut generic = 0_i32;
        while cursor < end {
            if self.punct(cursor, "<") {
                generic += 1;
            } else if self.punct(cursor, ">") {
                generic -= 1;
            } else if generic == 0 && self.punct(cursor, ":") {
                active = true;
            } else if active
                && generic == 0
                && self.kind(cursor) == Some(TokenKind::Identifier)
                && !self.punct(cursor.wrapping_sub(1), ".")
            {
                self.facts.references.push(Reference {
                    name: self.text(cursor).to_owned(),
                    kind: ReferenceKind::Implements,
                    receiver: None,
                    span: self.span(cursor, cursor),
                    owner: Some(owner.to_owned()),
                    string_arguments: Vec::new(),
                    name_arguments: Vec::new(),
                });
            }
            cursor += 1;
        }
    }

    /// A C or C++ function, which no keyword introduces: what marks it is a
    /// return type before the name and a body after the parameter list.
    ///
    /// Without this, `int add(int a, int b) { }` matched nothing and then fell
    /// through to the call path, so every C function definition was recorded
    /// as a call to itself - a self-edge in the graph, and no declaration for
    /// dead-code analysis to find.
    pub(super) fn typed_function(&mut self, index: usize, exported: bool) -> Option<usize> {
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
        let declaration = self.facts.declarations.len();
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: if owner.is_some() {
                DeclarationKind::Method
            } else {
                DeclarationKind::Function
            },
            span: declaration_span,
            extent: declaration_span,
            owner,
            // C has no visibility keyword; a static function is file-local and
            // everything else is linkable.
            exported: exported || !self.is_static(index),
        });
        self.scopes.push(Scope {
            name,
            depth: None,
            declaration: Some(declaration),
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
    pub(super) fn type_argument_span(&self, index: usize) -> usize {
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
    pub(super) fn type_argument_names(&self, index: usize, length: usize) -> Vec<String> {
        (index..index + length)
            .filter(|cursor| self.kind(*cursor) == Some(TokenKind::Identifier))
            .map(|cursor| self.text(cursor).to_owned())
            .collect()
    }

    /// Whether the declaration ending at `index` was marked `static`.
    pub(super) fn is_static(&self, index: usize) -> bool {
        let start = index.saturating_sub(4);
        (start..index).any(|cursor| self.text(cursor) == "static")
    }

    /// A method written directly inside a class or struct body, in languages
    /// that declare members without a keyword.
    pub(super) fn braced_member(&mut self, index: usize, exported: bool) -> Option<usize> {
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
        let declaration = self.facts.declarations.len();
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Method,
            span: declaration_span,
            extent: declaration_span,
            owner: self.owner(),
            exported,
        });
        // A member named like its enclosing type is a constructor, and its
        // parameter types are dependency-injection wiring.
        let constructor = self
            .scopes
            .last()
            .is_some_and(|scope| scope.type_body && scope.name == name);
        if constructor {
            self.parameter_type_uses(cursor + 2);
        }
        self.scopes.push(Scope {
            name,
            depth: None,
            declaration: Some(declaration),
            type_body: false,
            test_only,
        });
        Some(cursor + 1)
    }
}
