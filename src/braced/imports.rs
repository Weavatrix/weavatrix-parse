use super::{Extractor, Import, ImportBinding, TokenKind};

impl Extractor<'_, '_> {
    /// The module forms these languages write: `use a::b;`, `mod x;`,
    /// `import "path"`, grouped Go imports, `import a.b.C;`, `using X;`.
    // One pass keeps the shared statement boundaries beside the language
    // spellings; those fail-open limits are what prevent run-on imports.
    pub(super) fn import(&mut self, start: usize) -> Option<usize> {
        let (index, forwarding) = self.import_head(start)?;
        let word = self.text(index).to_owned();
        if self.punct(index + 1, "(") {
            return Some(self.grouped_import(index));
        }
        if word == "mod" {
            return self.module_import(index);
        }

        let mut bindings = Vec::new();
        let (cursor, specifier) = self.scan_import_specifier(index, &word, &mut bindings);
        let specifier = specifier.trim_end_matches([':', '.']).to_owned();
        if specifier.is_empty() {
            return None;
        }
        Self::add_default_use_binding(&word, &specifier, &mut bindings);
        self.record_import(index, cursor, specifier, bindings, forwarding);
        Some(cursor)
    }

    fn import_head(&self, start: usize) -> Option<(usize, bool)> {
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
        Some((index, forwarding))
    }

    fn grouped_import(&mut self, index: usize) -> usize {
        // A parenthesised block lists several paths, as Go writes them.
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
        cursor + 1
    }

    fn module_import(&mut self, index: usize) -> Option<usize> {
        // A `mod x { ... }` with a body defines the module here; only a
        // declaration without a body pulls in another file.
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
        Some(index + 2)
    }

    fn scan_import_specifier(
        &mut self,
        index: usize,
        word: &str,
        bindings: &mut Vec<ImportBinding>,
    ) -> (usize, String) {
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
                    *bindings = self.package_import_bindings(cursor, &specifier);
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
        (cursor, specifier)
    }

    fn add_default_use_binding(word: &str, specifier: &str, bindings: &mut Vec<ImportBinding>) {
        if word == "use" && bindings.is_empty() {
            let imported = specifier
                .rsplit("::")
                .next()
                .unwrap_or(specifier)
                .to_owned();
            if imported != "*" {
                bindings.push(ImportBinding {
                    local: imported.clone(),
                    imported,
                });
            }
        }
    }

    fn record_import(
        &mut self,
        index: usize,
        cursor: usize,
        specifier: String,
        bindings: Vec<ImportBinding>,
        forwarding: bool,
    ) {
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
    }

    pub(super) fn named_import_bindings(&self, start: usize, end: usize) -> Vec<ImportBinding> {
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

    pub(super) fn package_import_bindings(
        &self,
        path: usize,
        specifier: &str,
    ) -> Vec<ImportBinding> {
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
}
