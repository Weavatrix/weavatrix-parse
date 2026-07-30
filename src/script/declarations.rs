use super::{Declaration, DeclarationKind, Extractor, Scope, TokenKind};

impl Extractor<'_, '_> {
    /// Declarations in every form the language writes them.
    pub(super) fn declaration(&mut self, index: usize) -> Option<usize> {
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
    pub(super) fn is_arrow_function(&self, name_index: usize) -> bool {
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
    pub(super) fn class_member(&mut self, index: usize, exported: bool) -> Option<usize> {
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

    pub(super) fn object_literal_owner(&self, open: usize) -> Option<String> {
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
    pub(super) fn skip_initializer(&self, start: usize) -> usize {
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
}
