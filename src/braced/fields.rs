use super::{Declaration, DeclarationKind, Extractor, Reference, ReferenceKind, TokenKind};

impl Extractor<'_, '_> {
    /// A field written inside a type body: `private String name = value;`.
    ///
    /// The name is the last one before the terminator, which is what makes a
    /// generic type such as `Map<String, Order> index;` name `index` rather
    /// than one of its type arguments.
    pub(super) fn braced_field(&mut self, index: usize, exported: bool) -> Option<usize> {
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
        self.field_type_uses(index, at);
        Some(cursor)
    }

    /// The types a field is declared with couple the owning type to them:
    /// `private OrderService orders;` is dependency-injection wiring that no
    /// call site ever names, so losing it makes the provider look unused.
    fn field_type_uses(&mut self, start: usize, name_at: usize) {
        let mut cursor = start;
        while cursor < name_at {
            self.type_use(cursor);
            cursor += 1;
        }
    }

    /// Constructor parameters name the types a type is assembled from - the
    /// other half of dependency-injection wiring.
    pub(super) fn parameter_type_uses(&mut self, start: usize) {
        let limit = (start + 128).min(self.tokens.len());
        let mut depth = 1_i32;
        let mut cursor = start;
        while cursor < limit && depth > 0 {
            if self.punct(cursor, "(") {
                depth += 1;
            } else if self.punct(cursor, ")") {
                depth -= 1;
            } else {
                self.type_use(cursor);
            }
            cursor += 1;
        }
    }

    /// Records one capitalized, unqualified identifier as a type use. An
    /// annotation name (`@Autowired`) or a member segment (`x.Type`) is not a
    /// type position.
    fn type_use(&mut self, cursor: usize) {
        if self.kind(cursor) != Some(TokenKind::Identifier)
            || self.punct(cursor.wrapping_sub(1), ".")
            || self.punct(cursor.wrapping_sub(1), "@")
            || !self.text(cursor).starts_with(char::is_uppercase)
        {
            return;
        }
        self.facts.references.push(Reference {
            name: self.text(cursor).to_owned(),
            kind: ReferenceKind::Uses,
            receiver: None,
            span: self.span(cursor, cursor),
            owner: self.owner(),
            string_arguments: Vec::new(),
            name_arguments: Vec::new(),
        });
    }

    /// Every name declared inside a `const (...)` or `var (...)` group.
    ///
    /// Each line of the group declares one name, so the first identifier on a
    /// line is the declaration and the rest is its value.
    pub(super) fn grouped_declarations(
        &mut self,
        start: usize,
        open: usize,
        kind: DeclarationKind,
    ) -> usize {
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
            if self.is_grouped_declaration_name(cursor, line, parentheses, braces, brackets) {
                line = self.tokens[cursor].line;
                self.record_grouped_declaration(cursor, kind);
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

    fn is_grouped_declaration_name(
        &self,
        cursor: usize,
        previous_line: u32,
        parentheses: u32,
        braces: u32,
        brackets: u32,
    ) -> bool {
        parentheses == 1
            && braces == 0
            && brackets == 0
            && self.kind(cursor) == Some(TokenKind::Identifier)
            && self.tokens[cursor].line != previous_line
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
    }

    fn record_grouped_declaration(&mut self, cursor: usize, kind: DeclarationKind) {
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
    }
}
