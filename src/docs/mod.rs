//! Structural extraction for Markdown, MDX, `reStructuredText` and `AsciiDoc`.
//!
//! Prose has no token structure worth the name: a `"` is a quotation mark, not
//! a literal, and `//` is part of a URL. So this reads lines directly rather
//! than through the tokenizer, which is the honest model for the format even
//! though it is the opposite of what every other extractor here does.
//!
//! Two facts are worth having. A heading is a named anchor other documents
//! link to, and nesting one heading under another is the document's own table
//! of contents. A link to a path in the repository is a dependency exactly as
//! an import is - which is what turns a documentation tree into part of the
//! graph rather than a pile of files beside it.

use crate::facts::{Declaration, DeclarationKind, Facts, Import, Span};
use crate::syntax::Language;

/// Extracts structural facts from one document.
#[must_use]
pub fn extract(source: &str, language: Language) -> Facts {
    let mut state = Extractor {
        facts: Facts::default(),
        headings: Vec::new(),
        offset: 0,
    };
    state.run(source, language);
    state.facts
}

/// A heading whose section later headings may nest inside.
struct Heading {
    name: String,
    level: usize,
}

struct Extractor {
    facts: Facts,
    headings: Vec<Heading>,
    offset: usize,
}

impl Extractor {
    fn run(&mut self, source: &str, language: Language) {
        let lines = source.lines().collect::<Vec<_>>();
        let mut fenced = false;
        let mut script = String::new();
        for (number, line) in lines.iter().enumerate() {
            let start = self.offset;
            self.offset += line.len() + 1;
            let trimmed = line.trim();
            // A fence hides everything until it closes: code inside a block is
            // an example, and reading its links would invent dependencies.
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let span = Span {
                start,
                end: start + line.len(),
                line: u32::try_from(number + 1).unwrap_or(u32::MAX),
                column: 1,
                end_line: u32::try_from(number + 1).unwrap_or(u32::MAX),
                end_column: u32::try_from(line.len() + 1).unwrap_or(u32::MAX),
            };
            // MDX holds real JavaScript imports, which the script extractor
            // already reads correctly; gathering them keeps that one rule.
            if language == Language::Mdx
                && (trimmed.starts_with("import ") || trimmed.starts_with("export "))
            {
                script.push_str(line);
                script.push('\n');
                continue;
            }
            self.heading(trimmed, lines.get(number + 1).copied(), language, span);
            self.targets(line, language, span);
        }
        if !script.is_empty() {
            let inner = crate::script::extract(&script, Language::TypeScript);
            self.facts.imports.extend(inner.imports);
        }
    }

    /// Records a heading and nests it under the last shallower one.
    fn heading(&mut self, line: &str, next: Option<&str>, language: Language, span: Span) {
        let Some((level, name)) = read_heading(line, next, language) else {
            return;
        };
        while self
            .headings
            .last()
            .is_some_and(|heading| heading.level >= level)
        {
            self.headings.pop();
        }
        self.facts.declarations.push(Declaration {
            name: name.clone(),
            kind: DeclarationKind::Heading,
            span,
            extent: span,
            owner: self.headings.last().map(|heading| heading.name.clone()),
            // Every heading in a document is reachable by anchor.
            exported: true,
        });
        self.headings.push(Heading { name, level });
    }

    /// Records every path this line points at.
    fn targets(&mut self, line: &str, language: Language, span: Span) {
        for target in link_targets(line, language) {
            if !is_repository_path(&target) {
                continue;
            }
            self.facts.imports.push(Import {
                specifier: target,
                span,
                type_only: false,
                reexport: false,
                names: Vec::new(),
                bindings: Vec::new(),
            });
        }
    }
}

mod formats;

use formats::{is_repository_path, link_targets, read_heading};

#[cfg(test)]
mod tests;
