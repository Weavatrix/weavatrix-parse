use super::{Declaration, DeclarationKind, Extractor, Reference, ReferenceKind, Scope, TokenKind};

impl Extractor<'_, '_> {
    pub(super) fn declaration(&mut self, index: usize) -> Option<usize> {
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
    pub(super) fn heritage(&mut self, start: usize, owner: &str) {
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
}
