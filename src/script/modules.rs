use super::{
    BTreeMap, Extractor, Import, ImportBinding, Reference, ReferenceKind, TokenKind, is_method,
};

impl Extractor<'_, '_> {
    /// `import ... from 'x'`, `import 'x'`, `import('x')`, `require('x')`,
    /// `export ... from 'x'`. Multi-line forms work because the walk is over
    /// tokens rather than lines.
    pub(super) fn module_statement(&mut self, index: usize) -> Option<usize> {
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
    pub(super) fn local_reexport(&mut self, start: usize, open: usize) -> Option<usize> {
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
    pub(super) fn route_table(&mut self, index: usize) -> Option<usize> {
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
    pub(super) fn clause_bindings(&self, start: usize, end: usize) -> Vec<ImportBinding> {
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
    pub(super) fn leads_to_from(&self, index: usize) -> bool {
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
    pub(super) fn braced_types_only(&self, start: usize, end: usize) -> bool {
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

    pub(super) fn string_at(&self, index: usize) -> Option<String> {
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
}
